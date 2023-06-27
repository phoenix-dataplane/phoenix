use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use phoenix_api::rpc::{RpcId, TransportStatus};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use phoenix_api_policy_hello_acl::control_plane;

use phoenix_common::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageGeneral,
};

use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;

use phoenix_common::engine::datapath::RpcMessageTx;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::{create_log_file, HelloAclConfig};

use chrono::prelude::*;
use itertools::iproduct;

pub mod hello {
    include!("proto.rs");
}

fn hello_request_name_readonly(req: &hello::HelloRequest) -> String {
    let buf = &req.name as &[u8];
    String::from_utf8_lossy(buf).to_string().clone()
}

pub struct struct_acl {
    pub name: String,
    pub permission: String,
}
impl struct_acl {
    pub fn new(name: String, permission: String) -> struct_acl {
        struct_acl {
            name: name,
            permission: permission,
        }
    }
}

pub(crate) struct HelloAclEngine {
    pub(crate) node: DataPathNode,
    pub(crate) indicator: Indicator,
    pub(crate) config: HelloAclConfig,
    pub(crate) table_acl: Vec<struct_acl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for HelloAclEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "HelloAclEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                self.config = HelloAclConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(HelloAclEngine, node);

impl Decompose for HelloAclEngine {
    fn flush(&mut self) -> Result<usize> {
        let mut work = 0;
        while !self.tx_inputs()[0].is_empty() || !self.rx_inputs()[0].is_empty() {
            if let Progress(n) = self.check_input_queue()? {
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
        let mut collections = ResourceCollection::with_capacity(4);
        collections.insert("config".to_string(), Box::new(engine.config));
        (collections, engine.node)
    }
}

impl HelloAclEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<HelloAclConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let mut table_acl = Vec::new();
        table_acl.push(struct_acl {
            name: "Apple".to_string(),
            permission: "N".to_string(),
        });
        table_acl.push(struct_acl {
            name: "Banana".to_string(),
            permission: "Y".to_string(),
        });

        let engine = HelloAclEngine {
            node,
            indicator: Default::default(),
            config,
            table_acl,
        };
        Ok(engine)
    }
}

impl HelloAclEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            loop {
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
            }
            self.indicator.set_nwork(work);
            future::yield_now().await;
        }
    }
}

#[inline]
fn materialize_nocopy(msg: &RpcMessageTx) -> &hello::HelloRequest {
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_ref().unwrap() };
    return req;
}

impl HelloAclEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
                        let mut input = Vec::new();
                        input.push(msg);
                        let output: Vec<_> = iproduct!(input.iter(), self.table_acl.iter())
                            .map(|(msg, join)| {
                                let rpc_message = materialize_nocopy(&msg);
                                let conn_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.conn_id;
                                let call_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.call_id;
                                let rpc_id = RpcId::new(conn_id, call_id);
                                if hello_request_name_readonly(rpc_message) == join.name {
                                    if join.permission == "Y" {
                                        let error = EngineRxMessage::Ack(
                                            rpc_id,
                                            TransportStatus::Error(unsafe {
                                                NonZeroU32::new_unchecked(403)
                                            }),
                                        );
                                        RpcMessageGeneral::RxMessage(error)
                                    } else {
                                        let raw_ptr: *const hello::HelloRequest = rpc_message;
                                        let new_msg = RpcMessageTx {
                                            meta_buf_ptr: msg.meta_buf_ptr.clone(),
                                            addr_backend: raw_ptr.addr(),
                                        };
                                        RpcMessageGeneral::TxMessage(EngineTxMessage::RpcMessage(
                                            new_msg,
                                        ))
                                    }
                                } else {
                                    RpcMessageGeneral::Pass
                                }
                            })
                            .collect();

                        for msg in output {
                            match msg {
                                RpcMessageGeneral::TxMessage(msg) => {
                                    self.tx_outputs()[0].send(msg)?;
                                }
                                RpcMessageGeneral::RxMessage(msg) => {
                                    self.rx_outputs()[0].send(msg)?;
                                }
                                _ => {}
                            }
                        }
                    }
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Ok(Status::Disconnected);
            }
        }

        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineRxMessage::Ack(rpc_id, status) => {
                        // todo
                        self.rx_outputs()[0].send(EngineRxMessage::Ack(rpc_id, status))?;
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                    }
                    m => self.rx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Ok(Status::Disconnected);
            }
        }
        Ok(Progress(0))
    }
}
