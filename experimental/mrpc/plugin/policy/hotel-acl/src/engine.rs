//! This engine can only be placed at the sender side for now.
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use anyhow::{anyhow, Result};
use fnv::FnvHashMap as HashMap;
use futures::future::BoxFuture;

use phoenix_api::rpc::{RpcId, TransportStatus};
use phoenix_api_policy_hotel_acl::control_plane;

use phoenix_common::engine::datapath::message::{EngineRxMessage, EngineTxMessage, RpcMessageTx};
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::HotelAclConfig;

use minstant::Instant;
use rand::prelude::*;

pub mod reservation {
    // The string specified here must match the proto package name
    include!("reservation.rs");
}

pub(crate) struct HotelAclEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    pub(crate) outstanding_req_pool: HashMap<RpcId, Box<reservation::Request>>,

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
        "HotelAclEngine".to_owned()
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

        let mut collections = ResourceCollection::with_capacity(2);
        collections.insert(
            "outstanding_req_pool".to_string(),
            Box::new(engine.outstanding_req_pool),
        );
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
        let outstanding_req_pool = *local
            .remove("outstanding_req_pool")
            .unwrap()
            .downcast::<HashMap<RpcId, Box<reservation::Request>>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<HotelAclConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = HotelAclEngine {
            node,
            indicator: Default::default(),
            outstanding_req_pool,
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

/// Copy the RPC request to a private heap and returns the request.
#[inline]
fn materialize(msg: &RpcMessageTx) -> Box<reservation::Request> {
    let req_ptr = Unique::new(msg.addr_backend as *mut reservation::Request).unwrap();
    let req = unsafe { req_ptr.as_ref() };
    // returns a private_req
    Box::new(req.clone())
}

#[inline]
fn should_block(req: &reservation::Request) -> bool {
    log::trace!("req.customer_name: {}", req.customer_name);
    req.customer_name == "danyang"
}

impl HotelAclEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        // 1 clone
                        let private_req = materialize(&msg);
                        // 2 check should block
                        // yes: ACK with error, drop the data
                        // no: pass the cloned msg to the next engine, who drops the data?
                        // Should we Ack right after clone?
                        let conn_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.conn_id;
                        let call_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.call_id;
                        let rpc_id = RpcId::new(conn_id, call_id);
                        if should_block(&private_req) {
                            let error = EngineRxMessage::Ack(
                                rpc_id,
                                TransportStatus::Error(unsafe { NonZeroU32::new_unchecked(403) }),
                            );
                            self.rx_outputs()[0].send(error).unwrap_or_else(|e| {
                                log::warn!("error when bubbling up the error, send failed e: {}", e)
                            });
                            drop(private_req);
                        } else {
                            // We will release the request on private heap after the RPC adapter
                            // passes us an Ack.
                            let raw_ptr: *const reservation::Request = &*private_req;
                            self.outstanding_req_pool.insert(rpc_id, private_req);

                            let mut rng = rand::thread_rng();

                            let mut request_timestamp = Instant::now();
                            let mut request_credit = rng.gen_range(1..1000);

                            let new_msg = RpcMessageTx {
                                meta_buf_ptr: msg.meta_buf_ptr,
                                addr_backend: raw_ptr.addr(),
                                request_timestamp,
                                request_credit,
                            };
                            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(new_msg))?;
                        }
                    }
                    // XXX TODO(cjr): it is best not to reorder the message
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        // forward all rx msgs
        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                if let EngineRxMessage::Ack(rpc_id, _status) = m {
                    // remove private_req
                    self.outstanding_req_pool.remove(&rpc_id);
                }
                self.rx_outputs()[0].send(m)?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
