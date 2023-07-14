use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use phoenix_api::rpc::{RpcId, TransportStatus};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use phoenix_api_policy_file_logging::control_plane;

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
use crate::config::{create_log_file, FileLoggingConfig};

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub mod hello {
    include!("proto.rs");
}

pub struct struct_rpc_events_file {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub source: String,
    pub destination: String,
    pub rpc: String,
}
impl struct_rpc_events_file {
    pub fn new(
        timestamp: DateTime<Utc>,
        event_type: String,
        source: String,
        destination: String,
        rpc: String,
    ) -> struct_rpc_events_file {
        struct_rpc_events_file {
            timestamp: timestamp,
            event_type: event_type,
            source: source,
            destination: destination,
            rpc: rpc,
        }
    }
}

impl fmt::Display for struct_rpc_events_file {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.timestamp);
        write!(f, "{}", self.event_type);
        write!(f, "{}", self.source);
        write!(f, "{}", self.destination);
        write!(f, "{}", self.rpc);
        write!(f, "\n")
    }
}

pub(crate) struct FileLoggingEngine {
    pub(crate) node: DataPathNode,
    pub(crate) indicator: Indicator,
    pub(crate) config: FileLoggingConfig,
    pub(crate) file_rpc_events_file: File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for FileLoggingEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "FileLoggingEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                self.config = FileLoggingConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(FileLoggingEngine, node);

impl Decompose for FileLoggingEngine {
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

impl FileLoggingEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<FileLoggingConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let mut file_rpc_events_file = create_log_file();
        let engine = FileLoggingEngine {
            node,
            indicator: Default::default(),
            config,
            file_rpc_events_file,
        };
        Ok(engine)
    }
}

impl FileLoggingEngine {
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

impl FileLoggingEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
                        let mut input = Vec::new();
                        input.push(msg);
                        for event in input
                            .iter()
                            .map(|req| {
                                struct_rpc_events_file::new(
                                    Utc::now(),
                                    format!("{:?}", meta_ref.msg_type),
                                    format!("{:?}", meta_ref.conn_id),
                                    format!("{:?}", meta_ref.conn_id),
                                    format!("{}", req.addr_backend.clone()),
                                )
                            })
                            .collect::<Vec<_>>()
                        {
                            write!(self.file_rpc_events_file, "{}", event);
                        }
                        let output: Vec<_> = input
                            .iter()
                            .map(|req| {
                                RpcMessageGeneral::TxMessage(EngineTxMessage::RpcMessage(
                                    RpcMessageTx::new(
                                        req.meta_buf_ptr.clone(),
                                        req.addr_backend.clone(),
                                    ),
                                ))
                            })
                            .collect::<Vec<_>>();

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
