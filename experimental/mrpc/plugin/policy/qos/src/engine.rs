use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::binary_heap::BinaryHeap;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use minstant::Instant;
use nix::unistd::Pid;

use uapi_policy_qos::control_plane;

use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::datapath::EngineTxMessage;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use crate::config::QosConfig;
use crate::DatapathError;

thread_local! {
    static BUFFER: RefCell<BinaryHeap<Reverse<SloTaggedTxMessage>>> = RefCell::new(BinaryHeap::new());
}

#[derive(Debug)]
struct SloTaggedTxMessage {
    deadline: Instant,
    source: Pid,
    message: EngineTxMessage,
}

impl PartialOrd for SloTaggedTxMessage {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.deadline.partial_cmp(&other.deadline)
    }
}

impl Ord for SloTaggedTxMessage {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.deadline.cmp(&other.deadline)
    }
}

impl PartialEq for SloTaggedTxMessage {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}

impl Eq for SloTaggedTxMessage {}

pub(crate) struct QosEngine {
    pub(crate) node: DataPathNode,
    pub(crate) indicator: Indicator,

    pub(crate) client_pid: Pid,
    pub(crate) config: QosConfig,
}

impl_vertex_for_engine!(QosEngine, node);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for QosEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "QosEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig(latency_budget) => {
                self.config = QosConfig {
                    latency_budget_microsecs: latency_budget,
                };
            }
        }
        Ok(())
    }

    fn pre_detach(&mut self) -> Result<()> {
        BUFFER.with_borrow_mut(|buf| {
            let mut messages = buf.drain().collect::<Vec<_>>();
            let client_pid = self.client_pid;
            let drained = messages.drain_filter(|msg| msg.0.source == client_pid);
            for msg in drained {
                self.tx_outputs()[0].send(msg.0.message)?;
            }
            buf.extend(messages);
            Ok(())
        })
    }
}

impl Decompose for QosEngine {
    fn flush(&mut self) -> Result<()> {
        while let Ok(m) = self.tx_inputs()[0].try_recv() {
            self.tx_outputs()[0].send(m)?;
        }
        Ok(())
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(4);
        collections.insert("client_pid".to_string(), Box::new(engine.client_pid));
        collections.insert("config".to_string(), Box::new(engine.config));
        (collections, engine.node)
    }
}

impl QosEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let client_pid = *local
            .remove("client_pid")
            .unwrap()
            .downcast::<Pid>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<QosConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = QosEngine {
            node,
            indicator: Default::default(),
            client_pid,
            config,
        };
        Ok(engine)
    }
}

impl QosEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue, ~100ns
            loop {
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => {
                        self.pre_detach()?;
                        return Ok(());
                    }
                }
            }

            match self.check_buffer()? {
                Progress(n) => work += n,
                Status::Disconnected => unreachable!("check buffer should not return disconnected"),
            }

            // If there's pending receives, there will always be future work to do.
            self.indicator.set_nwork(work);

            future::yield_now().await;
        }
    }
}

impl QosEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let now = Instant::now();
                        if self.config.latency_budget_microsecs > 0 {
                            let deadline =
                                now + Duration::from_micros(self.config.latency_budget_microsecs);
                            let tagged = SloTaggedTxMessage {
                                deadline,
                                source: self.client_pid,
                                message: EngineTxMessage::RpcMessage(msg),
                            };
                            BUFFER.with_borrow_mut(|buf| {
                                buf.push(Reverse(tagged));
                            });
                        } else {
                            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                        }
                    }
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }

    fn check_buffer(&mut self) -> Result<Status, DatapathError> {
        BUFFER.with_borrow_mut(|buf| {
            if let Some(msg) = buf.peek() {
                if msg.0.source == self.client_pid {
                    let now = Instant::now();
                    if now > msg.0.deadline {
                        let msg = buf.pop().unwrap().0.message;
                        self.tx_outputs()[0].send(msg)?;
                        return Ok(Progress(1));
                    }
                }
            }
            Ok(Progress(0))
        })
    }
}
