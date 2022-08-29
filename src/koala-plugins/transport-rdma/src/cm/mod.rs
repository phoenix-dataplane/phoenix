use fnv::FnvHashMap as HashMap;
use std::collections::VecDeque;
use std::io;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::Ordering;
use std::time::Duration;

use interface::Handle;
use rdma::rdmacm;

use koala::log;
use koala::resource::{Error as ResourceError, ResourceTable};

use super::state::EventChannel;
use crate::ApiError;

pub(crate) mod engine;

pub(crate) struct CmEventManager {
    pub(crate) poll: mio::Poll,
    pub(crate) err_buffer: VecDeque<ApiError>,
}

impl CmEventManager {
    pub(crate) fn new() -> io::Result<Self> {
        Ok(CmEventManager {
            poll: mio::Poll::new()?,
            err_buffer: VecDeque::new(),
        })
    }

    /// Add the event channel to the PollSet.
    pub(crate) fn register_event_channel(
        &self,
        channel_handle: Handle,
        channel: &rdmacm::EventChannel,
    ) -> Result<(), ApiError> {
        self.poll
            .registry()
            .register(
                &mut mio::unix::SourceFd(&channel.as_raw_fd()),
                mio::Token(channel_handle.0 as _),
                mio::Interest::READABLE,
            )
            .map_err(ApiError::Mio)?;
        Ok(())
    }

    pub(crate) fn first_error(&mut self) -> Option<ApiError> {
        self.err_buffer.pop_front()
    }

    pub(crate) fn poll_cm_event_once(
        &mut self,
        event_channel_table: &ResourceTable<EventChannel>,
    ) -> Result<(), ApiError> {
        let mut events = mio::Events::with_capacity(1);
        self.poll
            .poll(&mut events, Some(Duration::from_millis(1)))
            .map_err(ApiError::Mio)?;

        if let Some(io_event) = events.iter().next() {
            let handle = Handle(io_event.token().0 as _);
            let event_channel = match event_channel_table.get(&handle) {
                Ok(ec) => ec,
                Err(ResourceError::NotFound) => {
                    // skip the error and only leave a warning message
                    log::warn!("event_channel_handle {:?} not found in the table", handle);
                    return Ok(());
                }
                Err(e) => {
                    return Err(e.into());
                }
            };

            // read one event
            let cm_event = event_channel.get_cm_event().map_err(ApiError::RdmaCm)?;
            if cm_event.event() == rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_DISCONNECTED {
                log::debug!("poll_cm_event_once get a disonnected event");
                // Call disconnect to ack
                if let Ok(0) = unsafe { &*cm_event.id().context() }.compare_exchange(
                    0,
                    1,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    log::debug!("passively call disconnect");
                    cm_event.id().disconnect().map_err(ApiError::RdmaCm)?;
                }

                // No need to add the event to event_channel since the QP will be destroyed
                return Ok(());
            }

            // reregister everytime to simulate level-trigger
            self.poll
                .registry()
                .reregister(
                    &mut mio::unix::SourceFd(&event_channel.as_raw_fd()),
                    io_event.token(),
                    mio::Interest::READABLE,
                )
                .map_err(ApiError::Mio)?;

            // Add CmEvent to event channel buffer
            event_channel.add_event(cm_event);

            return Ok(());
        }

        Err(ApiError::NoCmEvent)
    }
}
