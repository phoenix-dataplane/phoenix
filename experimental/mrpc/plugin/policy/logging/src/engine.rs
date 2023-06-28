//! main logic happens here
use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::future::BoxFuture;
use phoenix_api_policy_logging::control_plane;
use phoenix_common::engine::datapath::RpcMessageTx;
use std::io::Write;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use phoenix_common::engine::datapath::message::{EngineRxMessage, EngineTxMessage};

use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::{create_log_file, LoggingConfig};

pub mod hello {
    include!("proto.rs");
}

/// The internal state of an logging engine,
/// it contains some template fields like `node`, `indicator`,
/// a config field, in that case `LoggingConfig`
/// and other custome fields like `log_file
pub(crate) struct LoggingEngine {
    pub(crate) node: DataPathNode,
    pub(crate) indicator: Indicator,
    pub(crate) config: LoggingConfig,
    /// log_file is where the log will be written into
    /// it is temperoray, i.e. we don't store it when restart
    pub(crate) log_file: std::fs::File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

/// template
impl Engine for LoggingEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "LoggingEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                self.config = LoggingConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(LoggingEngine, node);

impl Decompose for LoggingEngine {
    /// flush will be called when we need to clean the transient state before decompose
    /// # return
    /// * `Result<usize>` - number of work drained from tx & rx queue
    fn flush(&mut self) -> Result<usize> {
        let mut work = 0;
        /// drain the rx & tx queue
        while !self.tx_inputs()[0].is_empty() || !self.rx_inputs()[0].is_empty() {
            if let Progress(n) = self.check_input_queue()? {
                work += n;
            }
        }
        self.log_file.flush()?;
        // file will automatically be closed when the engine is dropped
        Ok(work)
    }

    /// template
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

impl LoggingEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<LoggingConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let log_file = create_log_file();
        let engine = LoggingEngine {
            node,
            indicator: Default::default(),
            config,
            log_file,
        };
        Ok(engine)
    }
}

impl LoggingEngine {
    async fn mainloop(&mut self) -> EngineResult {
        // open a write buffer to a file
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

#[inline]
fn materialize_nocopy(msg: &RpcMessageTx) -> &hello::HelloRequest {
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_ref().unwrap() };
    return req;
}

impl LoggingEngine {
    /// main logic about handling rx & tx input messages
    /// note that a logging engine can be deployed in client-side or server-side
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        // tx logic
        // For server it is `On-Response` logic, when sending  response to network
        // For client it is `On-Request` logic, when sending request to network
        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    // we care only log RPCs
                    // other types like ACK should not be logged since they are not
                    // ACKs between Client/Server, but communication between engines
                    // "Real" ACKs are logged in rx logic
                    EngineTxMessage::RpcMessage(msg) => {
                        // we get the metadata of RPC from the shared memory
                        let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
                        let rpc_message = materialize_nocopy(&msg);
                        // write the metadata into the file
                        // since meta_ref implements Debug, we can use {:?}
                        // rather than manully parse the metadata struct
                        write!(
                            self.log_file,
                            "{}{}{}{}{}\n",
                            Utc::now(),
                            format!("{:?}", meta_ref.msg_type),
                            format!("{:?}", meta_ref.conn_id),
                            format!("{:?}", meta_ref.conn_id),
                            format!("{}", String::from_utf8_lossy(&rpc_message.name)),
                        )
                        .unwrap();

                        // after logging, we forward the message to the next engine
                        self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                    }
                    // if received message is not RPC, we simple forward it
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Ok(Status::Disconnected);
            }
        }

        // tx logic
        // For server it is `On-Request` logic, when recving request from network
        // For client it is `On-Response` logic, when recving response from network
        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    // ACK means that
                    // If I am client: server received my request
                    // If I am server: client recevied my response
                    EngineRxMessage::Ack(rpc_id, status) => {
                        // log the info to the file
                        // forward the message
                        self.rx_outputs()[0].send(EngineRxMessage::Ack(rpc_id, status))?;
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        // forward the message
                        // again, this RpcMessage is not the application-level rpc
                        // so we don log them
                        self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                    }
                    // forward other unknown msg
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
