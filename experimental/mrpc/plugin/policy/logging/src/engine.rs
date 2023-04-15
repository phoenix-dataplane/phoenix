use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use std::io::Write;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use phoenix_api_policy_logging::control_plane;

use phoenix_common::engine::datapath::message::{EngineRxMessage, EngineTxMessage};

use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::{create_log_file, LoggingConfig};

pub(crate) struct LoggingEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,
    pub(crate) config: LoggingConfig,
    pub(crate) log_file: std::fs::File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

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
    fn flush(&mut self) -> Result<()> {
        while !self.tx_inputs()[0].is_empty() {
            self.check_input_queue()?;
        }
        self.log_file.flush()?;
        // file wll automatically be closed when the engine is dropped
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

impl LoggingEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
                        //log::info!("Got message on tx queue: {:?}", meta_ref);
                        self.log_file
                            .write(format!("Got message on tx queue: {:?}", meta_ref).as_bytes())
                            .expect("error writing to log file");
                        self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                    }
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // log::info!("Disconnected!");
                return Ok(Status::Disconnected);
            }
        }

        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineRxMessage::Ack(rpc_id, status) => {
                        self.log_file
                            .write(
                                format!(
                                    "Got ack on rx queue, rpc_id {:?}, status: {:?}",
                                    rpc_id, status
                                )
                                .as_bytes(),
                            )
                            .expect("error writing to log file");
                        self.rx_outputs()[0].send(EngineRxMessage::Ack(rpc_id, status))?;
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        //log::info!("Got msg on rx queue: {:?}", msg);
                        self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                    }
                    m => self.rx_outputs()[0].send(m)?,
                }
                //log::info!("Send msg in rx queue");
                //self.rx_outputs()[0].send(msg)?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // log::info!("Disconnected!");
                return Ok(Status::Disconnected);
            }
        }

        Ok(Progress(0))
    }
}
