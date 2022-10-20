use std::collections::VecDeque;
use std::mem;
use std::num::NonZeroU32;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use interface::engine::SchedulingMode;
use interface::{WcOpcode, WcStatus};
use ipc::transport::tcp::{cmd, dp};

use super::module::CustomerType;
use super::ops::Ops;
use super::{Error, TransportError};

use phoenix::engine::datapath::node::DataPathNode;
use phoenix::engine::{future, Decompose, Engine, EngineResult, Indicator};
use phoenix::envelop::ResourceDowncast;
use phoenix::impl_vertex_for_engine;
use phoenix::module::{ModuleCollection, Version};
use phoenix::storage::{ResourceCollection, SharedStorage};
use phoenix::{log, tracing};

pub(crate) struct TransportEngine {
    pub(crate) customer: CustomerType,
    pub(crate) indicator: Indicator,

    pub(crate) node: DataPathNode,
    pub(crate) ops: Ops,
    pub(crate) cq_err_buffer: VecDeque<dp::Completion>,
    pub(crate) _mode: SchedulingMode,
}

impl_vertex_for_engine!(TransportEngine, node);

impl Decompose for TransportEngine {
    #[inline]
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;
        let mut collections = ResourceCollection::with_capacity(5);
        tracing::trace!("dumping RdmaTransport-TransportEngine states...");
        collections.insert("customer".to_string(), Box::new(engine.customer));
        collections.insert("mode".to_string(), Box::new(engine._mode));
        collections.insert("ops".to_string(), Box::new(engine.ops));
        collections.insert("cq_err_buffer".to_string(), Box::new(engine.cq_err_buffer));
        (collections, engine.node)
    }
}

impl TransportEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        tracing::trace!("restoring TcpTransport-TransportEngine states...");
        let customer = *local
            .remove("customer")
            .unwrap()
            .downcast::<CustomerType>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let mode = *local
            .remove("mode")
            .unwrap()
            .downcast::<SchedulingMode>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let ops = *local
            .remove("ops")
            .unwrap()
            .downcast::<Ops>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let cq_err_buffer = *local
            .remove("cq_err_buffer")
            .unwrap()
            .downcast::<VecDeque<dp::Completion>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = TransportEngine {
            customer,
            indicator: Default::default(),
            _mode: mode,
            node,
            ops,
            cq_err_buffer,
        };
        Ok(engine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for TransportEngine {
    fn description(self: Pin<&Self>) -> String {
        format!("TCP TransportEngine, user: {:?}", self.ops.state.shared.pid)
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }
}

impl TransportEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            // let mut timer = phoenix::timer::Timer::new();
            let mut nwork = 0;
            if let Progress(n) = self.check_dp(false)? {
                nwork += n;
            }
            // timer.tick();

            if let Status::Disconnected = self.check_cmd().await? {
                return Ok(());
            }
            // timer.tick();

            self.check_comp();
            // timer.tick();

            // log::info!("TcpTransport mainloop: {}", timer);

            self.indicator.set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl TransportEngine {
    fn flush_dp(&mut self) -> Result<Status, TransportError> {
        let mut processed = 0;
        let existing_work = self.customer.get_avail_wr_count()?;

        while processed < existing_work {
            if let Progress(n) = self.check_dp(true)? {
                processed += n;
            }
        }

        Ok(Progress(processed))
    }

    fn check_dp(&mut self, flushing: bool) -> Result<Status, TransportError> {
        use dp::WorkRequest;
        const BUF_LEN: usize = 32;

        // Fetch available work requests. Copy them into a buffer.
        let max_count = if !flushing {
            BUF_LEN.min(self.customer.get_avail_wc_slots()?)
        } else {
            BUF_LEN
        };
        if max_count == 0 {
            return Ok(Progress(0));
        }

        // TODO(cjr): flamegraph shows that a large portion of time is spent in this with_capacity
        // optimize this with smallvec or so.
        let mut count = 0;
        let mut buffer = Vec::with_capacity(BUF_LEN);

        self.customer
            .dequeue_wr_with(|ptr, read_count| unsafe {
                // TODO(cjr): One optimization is to post all available send requests in one batch
                // using doorbell
                debug_assert!(max_count <= BUF_LEN);
                count = max_count.min(read_count);
                for i in 0..count {
                    buffer.push(ptr.add(i).cast::<WorkRequest>().read());
                }
                count
            })
            .unwrap_or_else(|e| panic!("check_dp: {}", e));

        // Process the work requests.
        for wr in &buffer {
            let result = self.process_dp(wr, flushing);
            match result {
                Ok(()) => {}
                Err(e) => {
                    // NOTE(cjr): Typically, we expect to report the error to the user right after
                    // we get this error. But busy waiting here may cause circular waiting between
                    // phoenix engine and the user in some circumstance.
                    //
                    // The work queue and completion queue are both bounded. The bound is set by
                    // phoenix system rather than specified by the user. Imagine that the user is
                    // trying to post_send without polling for completion timely. The completion
                    // queue is full and phoenix will spin here without making any progress (e.g.
                    // drain the work queue).
                    //
                    // Therefore, we put the error into a local buffer. Whenever we want to put
                    // stuff in the shared memory completion queue, we put from the local buffer
                    // first. This way seems perfect. It can guarantee progress. The backpressure
                    // is also not broken.
                    let _sent = self.process_dp_error(wr, e).unwrap();
                }
            }
        }

        self.try_flush_cq_err_buffer().unwrap();

        Ok(Progress(count))
    }

    async fn check_cmd(&mut self) -> Result<Status, Error> {
        let ret = self.customer.try_recv_cmd();
        match ret {
            // handle request
            Ok(req) => {
                // Flush datapath!
                self.flush_dp()?;
                let result = self.process_cmd(&req).await;
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Err(e) => {
                        // better to log the error here, in case sometimes the customer does
                        // not receive the error
                        log::error!("process_cmd error: {}", e);
                        self.customer.send_comp(cmd::Completion(Err(e.into())))?
                    }
                }
                Ok(Progress(1))
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                Ok(Progress(0))
            }
            Err(ipc::TryRecvError::Disconnected) => Ok(Status::Disconnected),
            Err(ipc::TryRecvError::Other(_e)) => Err(Error::IpcTryRecv),
        }
    }

    fn get_completion_from_error(&self, wr: &dp::WorkRequest, e: TransportError) -> dp::Completion {
        use dp::WorkRequest;
        let (sock_handle, wr_id) = match wr {
            WorkRequest::PostSend(sock_handle, wr_id, ..)
            | WorkRequest::PostRecv(sock_handle, wr_id, ..) => (*sock_handle, *wr_id),
            WorkRequest::PollCq(sock_handle) => (*sock_handle, 0),
        };
        dp::Completion {
            wr_id,
            conn_id: sock_handle.0 as _,
            opcode: WcOpcode::Invalid,
            status: WcStatus::Error(NonZeroU32::new(e.into_vendor_err()).unwrap()),
            buf: ipc::buf::Range { offset: 0, len: 0 },
            byte_len: 0,
            imm: 0,
        }
    }

    /// Return the error through the work completion. The error happened in phoenix
    /// side is considered a `vendor_err`.
    ///
    /// NOTE(cjr): There's no fundamental difference between the failure on
    /// post_send and the failure on poll_cq for the same work request.
    /// The general practice is to return the error early, but we can
    /// postpone the error returning in order to achieve asynchronous IPC.
    ///
    /// However, the completion order can be changed due to some errors
    /// happen on request posting stage.
    fn process_dp_error(
        &mut self,
        wr: &dp::WorkRequest,
        e: TransportError,
    ) -> Result<bool, TransportError> {
        let mut sent = false;
        let comp = self.get_completion_from_error(wr, e);
        if !self.cq_err_buffer.is_empty() {
            self.cq_err_buffer.push_back(comp);
        } else {
            self.customer.notify_wc_with(|ptr, _count| unsafe {
                // construct an WorkCompletion and set the vendor_err
                ptr.cast::<dp::Completion>().write(comp);
                sent = true;
                1
            })?;
        }

        Ok(sent)
    }

    fn try_flush_cq_err_buffer(&mut self) -> Result<(), TransportError> {
        if self.cq_err_buffer.is_empty() {
            return Ok(());
        }

        let mut cq_err_buffer = VecDeque::new();
        mem::swap(&mut cq_err_buffer, &mut self.cq_err_buffer);
        let status = self.customer.notify_wc_with(|ptr, count| unsafe {
            let mut cnt = 0;
            // construct an WorkCompletion and set the vendor_err
            for comp in cq_err_buffer.drain(..count) {
                ptr.cast::<dp::Completion>().write(comp);
                cnt += 1;
            }
            cnt
        });

        mem::swap(&mut cq_err_buffer, &mut self.cq_err_buffer);
        status?;
        Ok(())
    }

    fn process_dp(&mut self, req: &dp::WorkRequest, _flushing: bool) -> Result<(), TransportError> {
        use dp::WorkRequest;
        match req {
            WorkRequest::PostSend(sock_handle, wr_id, range, imm) => {
                self.ops.post_send(*sock_handle, *wr_id, *range, *imm)?;
                Ok(())
            }
            WorkRequest::PostRecv(sock_handle, wr_id, range) => {
                self.ops.post_recv(*sock_handle, *wr_id, *range)?;
                Ok(())
            }
            WorkRequest::PollCq(_sock_handle) => {
                unimplemented!("PollCq");
            }
        }
    }

    async fn process_cmd(&mut self, req: &cmd::Command) -> Result<cmd::CompletionKind, Error> {
        use cmd::{Command, CompletionKind};
        match req {
            Command::Bind(addr, _backlog) => {
                let handle = self.ops.bind(addr)?;
                Ok(CompletionKind::Bind(handle))
            }
            Command::Accept(listener) => {
                let handle = self.ops.accept(*listener)?;
                Ok(CompletionKind::Accept(handle))
            }
            Command::Connect(addr) => {
                let handle = self.ops.connect(addr)?;
                Ok(CompletionKind::Connect(handle))
            }
            Command::RegMr(_nbytes) => {
                unimplemented!("RegMr");
            }
            Command::SetSockOption(_handle) => {
                unimplemented!("SetSockOption");
                // Ok(CompletionKind::SetSockOption)
            }
        }
    }

    fn check_comp(&self) {
        unimplemented!("check_comp");
    }
}
