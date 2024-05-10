//! This engine can only be placed at the sender side for now.
use std::os::unix::net::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use phoenix_api::rpc::{RpcId, StatusCode};
use phoenix_api_policy_hello_acl_receiver::control_plane;

use phoenix_common::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageRx, RpcMessageTx,
};
use phoenix_common::engine::datapath::meta_pool::MetaBufferPool;
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::HelloAclReceiverConfig;

pub mod hello {
    // The string specified here must match the proto package name
    include!("rpc_hello.rs");
}

pub(crate) struct HelloAclReceiverEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    pub(crate) meta_buf_pool: MetaBufferPool,
    pub(crate) config: HelloAclReceiverConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for HelloAclReceiverEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "HelloAclReceiverEngine".to_owned()
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
                self.config = HelloAclReceiverConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(HelloAclReceiverEngine, node);

impl Decompose for HelloAclReceiverEngine {
    fn flush(&mut self) -> Result<usize> {
        let mut work = 0;
        while !self.tx_inputs()[0].is_empty() || !self.rx_inputs()[0].is_empty() {
            if let Progress(n) = self.receiver_check_input_queue()? {
                work += n;
            }
        }
        Ok(work)
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(2);
        collections.insert("meta_buf_pool".to_string(), Box::new(engine.meta_buf_pool));

        collections.insert("config".to_string(), Box::new(engine.config));

        (collections, engine.node)
    }
}

impl HelloAclReceiverEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<HelloAclReceiverConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let meta_buf_pool = *local
            .remove("meta_buf_pool")
            .unwrap()
            .downcast::<MetaBufferPool>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let engine = HelloAclReceiverEngine {
            node,
            indicator: Default::default(),
            meta_buf_pool,
            config,
        };
        Ok(engine)
    }
}

impl HelloAclReceiverEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue, ~100ns
            loop {
                match self.receiver_check_input_queue()? {
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
fn materialize_rx(msg: &RpcMessageRx) -> Box<hello::HelloRequest> {
    let req_ptr = Unique::new(msg.addr_backend as *mut hello::HelloRequest).unwrap();
    let req = unsafe { req_ptr.as_ref() };
    // returns a private_req
    Box::new(req.clone())
}

#[inline]
fn should_block(req: &hello::HelloRequest) -> bool {
    let buf = &req.name as &[u8];
    return buf == b"Apple";
    //let name = String::from_utf8_lossy(buf);
    //log::info!("raw: {:?}, req.name: {:?}", buf, name);
}

impl HelloAclReceiverEngine {
    fn receiver_check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                self.tx_outputs()[0].send(msg)?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        // forward all rx msgs
        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                match m {
                    EngineRxMessage::Ack(rpc_id, _status) => {
                        if let Ok(()) = self.meta_buf_pool.release(rpc_id) {
                            // log::info!(
                            //     "Access denied ack received, rpc_id: {:?} metabuf released",
                            //     rpc_id
                            // );
                        } else {
                            // log::info!("release failed!: {:?}", rpc_id);
                            self.rx_outputs()[0].send(m)?;
                        }
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        //log::debug!("HelloAclReceiverEngine: rx msg_meta: {:?}", msg.meta);
                        // check whether the request should be blocked
                        let private_req = materialize_rx(&msg);
                        if should_block(&private_req) {
                            // We need to copy meta, add it to meta_buf_pool, and send it as the tx msg
                            // Is there better way to do this and avoid unsafe?
                            let mut meta = unsafe { msg.meta.as_ref().clone() };
                            meta.status_code = StatusCode::AccessDenied;
                            let mut meta_ptr = self
                                .meta_buf_pool
                                .obtain(RpcId(meta.conn_id, meta.call_id))
                                .expect("meta_buf_pool is full");
                            unsafe {
                                meta_ptr.as_meta_ptr().write(meta);
                                meta_ptr.0.as_mut().num_sge = 0;
                                meta_ptr.0.as_mut().value_len = 0;
                            }
                            let rpc_msg = RpcMessageTx {
                                meta_buf_ptr: meta_ptr,
                                addr_backend: 0,
                            };
                            let new_msg = EngineTxMessage::RpcMessage(rpc_msg);
                            self.tx_outputs()[0]
                                .send(new_msg)
                                .expect("send new message error");
                            let msg_call_ids =
                                [meta.call_id, meta.call_id, meta.call_id, meta.call_id];
                            self.tx_outputs()[0].send(EngineTxMessage::ReclaimRecvBuf(
                                meta.conn_id,
                                msg_call_ids,
                            ))?;
                        } else {
                            self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                        }
                    }
                    EngineRxMessage::RecvError(_, _) => {
                        self.rx_outputs()[0].send(m)?;
                    }
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
