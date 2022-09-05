//! This engine can only be placed at the sender side for now.
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use interface::rpc::{RpcId, TransportStatus};
use ipc::hotel_acl::control_plane;

use koala::engine::datapath::message::{EngineRxMessage, EngineTxMessage, RpcMessageTx};
use koala::engine::datapath::node::DataPathNode;
use koala::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use koala::envelop::ResourceDowncast;
use koala::impl_vertex_for_engine;
use koala::log;
use koala::module::Version;
use koala::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::HotelAclConfig;

pub mod reservation {
    // The string specified here must match the proto package name
    include!("reservation.rs");
}
use reservation::Request;

pub(crate) struct HotelAclEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    // A set of func_ids to apply the rate limit.
    // TODO(cjr): maybe put this filter in a separate engine like FilterEngine/ClassiferEngine.
    // pub(crate) filter: FnvHashSet<u32>,
    // Number of tokens to add for each seconds.
    pub(crate) config: HotelAclConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for HotelAclEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        format!("HotelAclEngine")
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig => {
                // Update config
                self.config = HotelAclConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(HotelAclEngine, node);

impl Decompose for HotelAclEngine {
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(4);
        collections.insert("config".to_string(), Box::new(engine.config));
        (collections, engine.node)
    }
}

impl HotelAclEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<HotelAclConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = HotelAclEngine {
            node,
            indicator: Default::default(),
            config,
        };
        Ok(engine)
    }
}

impl HotelAclEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue, ~100ns
            loop {
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
            }

            // If there's pending receives, there will always be future work to do.
            self.indicator.set_nwork(work);

            future::yield_now().await;
        }
    }
}

impl HotelAclEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use koala::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        if self.should_block(&msg) {
                            let conn_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.conn_id;
                            let call_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.call_id;
                            let rpc_id = RpcId::new(conn_id, call_id);
                            let error = EngineRxMessage::Ack(
                                rpc_id,
                                TransportStatus::Error(unsafe { NonZeroU32::new_unchecked(403) }),
                            );
                            self.rx_outputs()[0].send(error).unwrap_or_else(|e| {
                                log::warn!("error when bubbling up the error, send failed e: {}", e)
                            });
                        } else {
                            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                        }
                    }
                    // XXX TODO(cjr): it is best not to reorder the message
                    m @ _ => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        // forward all rx msgs
        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                self.rx_outputs()[0].send(m)?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }

    fn should_block(&self, msg: &RpcMessageTx) -> bool {
        let req_ptr = Unique::new(msg.addr_backend as *mut Request).unwrap();
        let req = unsafe { req_ptr.as_ref() };
        // TODO(cjr): clone on access
        // let private_req = req.clone();
        use std::ops::Deref;
        log::trace!("req.customer_name: {}", req.customer_name.deref());
        req.customer_name.deref() == "danyang"
    }
}
