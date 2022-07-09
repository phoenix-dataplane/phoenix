use fnv::FnvHashMap as HashMap;
use std::collections::VecDeque;
use std::io;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use interface::Handle;
use rdma::rdmacm;

use super::ApiError;
use crate::resource::ResourceTable;

pub(crate) mod engine;

pub(crate) struct CmEventManager {
    pub(crate) poll: mio::Poll,
    // event_channel_handle -> EventPool
    event_channel_buffer: HashMap<interface::Handle, VecDeque<rdmacm::CmEvent>>,

    pub(crate) err_buffer: VecDeque<ApiError>,
}

impl CmEventManager {
    pub(crate) fn new() -> io::Result<Self> {
        Ok(CmEventManager {
            poll: mio::Poll::new()?,
            event_channel_buffer: HashMap::default(),
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

    /// Get an event that matches the event type.
    ///
    /// If there's any event matches, return the first one matched. Otherwise, returns None.
    pub(crate) fn get_one_cm_event(
        &mut self,
        event_channel_handle: &Handle,
        event_type: rdma::ffi::rdma_cm_event_type::Type,
    ) -> Option<rdmacm::CmEvent> {
        let event_queue = self
            .event_channel_buffer
            .entry(*event_channel_handle)
            .or_insert_with(VecDeque::default);

        if let Some(pos) = event_queue.iter().position(|e| e.event() == event_type) {
            event_queue.remove(pos)
        } else {
            None
        }
    }

    pub(crate) fn first_error(&mut self) -> Option<ApiError> {
        self.err_buffer.pop_front()
    }

    pub(crate) fn poll_cm_event_once(
        &mut self,
        event_channel_table: &ResourceTable<rdmacm::EventChannel>,
    ) -> Result<(), ApiError> {
        let mut events = mio::Events::with_capacity(1);
        self.poll
            .poll(&mut events, Some(Duration::from_millis(1)))
            .map_err(ApiError::Mio)?;

        if let Some(io_event) = events.iter().next() {
            let handle = Handle(io_event.token().0 as _);
            let event_channel = event_channel_table.get(&handle)?;

            // read one event
            let cm_event = event_channel.get_cm_event().map_err(ApiError::RdmaCm)?;
            if cm_event.event() == rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_DISCONNECTED {
                log::warn!("poll_cm_event_once get a disonnected event");
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

            self.event_channel_buffer
                .entry(handle)
                .or_insert_with(VecDeque::default)
                .push_back(cm_event);

            return Ok(());
        }

        Err(ApiError::NoCmEvent)
    }
}
