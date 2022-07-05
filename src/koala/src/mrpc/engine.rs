use std::future::Future;
use std::mem;
use std::path::PathBuf;

use interface::rpc::{MessageErased, RpcId, TransportStatus};

use interface::engine::SchedulingMode;
use ipc::mrpc::{cmd, control_plane, dp};

use super::meta_pool::MetaBufferPool;
use super::module::CustomerType;
use super::state::State;
use super::{DatapathError, Error};
use crate::engine::graph::{EngineTxMessage, RpcMessageTx};
use crate::engine::{future, Engine, EngineResult, EngineRxMessage, Indicator, Vertex};
use crate::mrpc::builder::build_dispatch_library;
use crate::node::Node;

pub struct MrpcEngine {
    pub(crate) _state: State,

    pub(crate) customer: CustomerType,
    pub(crate) node: Node,
    pub(crate) cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
    pub(crate) cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,

    // mRPC private buffer pools
    /// Buffer pool for meta and eager message
    pub(crate) meta_buf_pool: MetaBufferPool,

    pub(crate) _mode: SchedulingMode,

    pub(crate) dispatch_build_cache: PathBuf,

    pub(crate) transport_type: Option<control_plane::TransportType>,

    pub(crate) indicator: Option<Indicator>,
    pub(crate) wr_read_buffer: Vec<dp::WorkRequest>,
}

crate::unimplemented_ungradable!(MrpcEngine);
crate::impl_vertex_for_engine!(MrpcEngine, node);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for MrpcEngine {
    type Future = impl Future<Output = EngineResult>;

    fn description(&self) -> String {
        format!("MrpcEngine, todo show more information")
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }

    fn entry(mut self) -> Self::Future {
        Box::pin(async move { self.mainloop().await })
    }
}

impl MrpcEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            // let mut timer = crate::timer::Timer::new();
            let mut nwork = 0;

            // 400-900ns ->
            // no work: 40ns
            // has work: 200-1000ns
            if let Progress(n) = self.check_customer()? {
                nwork += n;
            }
            // timer.tick();

            // no work: 100-150ns
            // has work: 300-600ns
            match self.check_input_queue()? {
                Progress(n) => nwork += n,
                Status::Disconnected => break,
            }
            // timer.tick();

            // 80-100ns, sometimes 200ns
            if let Status::Disconnected = self.check_cmd().await? {
                break;
            }
            // timer.tick();

            // 50ns
            self.check_new_incoming_connection()?;
            self.indicator.as_ref().unwrap().set_nwork(nwork);
            // timer.tick();
            // log::info!("mrpc mainloop: {}", timer);

            future::yield_now().await;
        }

        self.wait_outstanding_complete().await?;
        return Ok(());
    }
}

impl MrpcEngine {
    // we need to wait for RpcAdapter engine to finish outstanding send requests
    // (whether successful or not), to release message meta pool and shutdown mRPC engine.
    // However, we cannot indefinitely wait for it in case of wc errors.
    async fn wait_outstanding_complete(&mut self) -> Result<(), DatapathError> {
        use crate::engine::graph::TryRecvError;
        while !self.meta_buf_pool.is_full() {
            match self.rx_inputs()[0].try_recv() {
                Ok(msg) => match msg {
                    EngineRxMessage::Ack(rpc_id, _status) => {
                        // release the buffer whatever the status is.
                        self.meta_buf_pool.release(rpc_id)?;
                    }
                    EngineRxMessage::RpcMessage(_) => {}
                },
                Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => {}
            }
            future::yield_now().await;
        }
        Ok(())
    }

    async fn check_cmd(&mut self) -> Result<Status, Error> {
        match self.customer.try_recv_cmd() {
            // handle request
            Ok(req) => {
                let result = self.process_cmd(&req).await;
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Err(e) => self.customer.send_comp(cmd::Completion(Err(e.into())))?,
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

    fn create_transport(&mut self, transport_type: control_plane::TransportType) {
        self.transport_type = Some(transport_type);
    }

    async fn process_cmd(&mut self, req: &cmd::Command) -> Result<cmd::CompletionKind, Error> {
        use ipc::mrpc::cmd::{Command, CompletionKind};
        match req {
            Command::SetTransport(transport_type) => {
                if self.transport_type.is_some() {
                    Err(Error::TransportType)
                } else {
                    self.create_transport(*transport_type);
                    Ok(CompletionKind::SetTransport)
                }
            }
            Command::Connect(addr) => {
                self.cmd_tx.send(Command::Connect(*addr)).unwrap();
                match self.cmd_rx.recv().await.unwrap().0 {
                    Ok(CompletionKind::ConnectInternal(handle, recv_mrs, fds)) => {
                        self.customer.send_fd(&fds).unwrap();
                        Ok(CompletionKind::Connect((handle, recv_mrs)))
                    }
                    other => panic!("unexpected: {:?}", other),
                }
            }
            Command::Bind(addr) => {
                self.cmd_tx.send(Command::Bind(*addr)).unwrap();
                match self.cmd_rx.recv().await.unwrap().0 {
                    Ok(CompletionKind::Bind(listener_handle)) => {
                        // just forward it
                        Ok(CompletionKind::Bind(listener_handle))
                    }
                    other => panic!("unexpected: {:?}", other),
                }
            },
            Command::UpdateProtos(protos) => {
                let dylib_path = build_dispatch_library(protos.clone(), self.dispatch_build_cache.clone())?;
                self.cmd_tx.send(Command::UpdateProtosInner(dylib_path)).unwrap();
                match self.cmd_rx.recv().await.unwrap().0 {
                    Ok(CompletionKind::UpdateProtos) => {
                        // just forward it
                        Ok(CompletionKind::UpdateProtos)
                    }
                    other => panic!("unexpected: {:?}", other),
                }
            },
            Command::UpdateProtosInner(_) => {
                panic!("UpdateProtosInner is only used in backend")
            }
        }
    }

    fn check_customer(&mut self) -> Result<Status, DatapathError> {
        use dp::WorkRequest;
        let buffer_cap = self.wr_read_buffer.capacity();
        // let mut timer = crate::timer::Timer::new();

        // 15ns
        // Fetch available work requests. Copy them into a buffer.
        let max_count = buffer_cap.min(self.customer.get_avail_wc_slots()?);
        if max_count == 0 {
            return Ok(Progress(0));
        }
        // timer.tick();

        // 300-10us, mostly 300ns (Vec::with_capacity())
        let mut count = 0;
        self.wr_read_buffer.clear();

        // timer.tick();

        // 60-150ns
        self.customer
            .dequeue_wr_with(|ptr, read_count| unsafe {
                // TODO(cjr): max_count <= read_count always holds
                count = max_count.min(read_count);
                for i in 0..count {
                    self.wr_read_buffer
                        .push(ptr.add(i).cast::<WorkRequest>().read());
                }
                count
            })
            .unwrap_or_else(|e| panic!("check_customer: {}", e));

        // Process the work requests.
        // timer.tick();

        // no work: 10ns
        // has work: 100-400ns
        let buffer = mem::take(&mut self.wr_read_buffer);

        for wr in &buffer {
            let ret = self.process_dp(wr);
            match ret {
                Ok(()) => {}
                Err(e) => {
                    self.wr_read_buffer = buffer;
                    // TODO(cjr): error handling
                    return Err(e);
                }
            }
        }

        self.wr_read_buffer = buffer;

        // timer.tick();
        // log::info!("check_customer: {}", timer);

        Ok(Progress(count))
    }

    fn process_dp(&mut self, req: &dp::WorkRequest) -> Result<(), DatapathError> {
        use dp::WorkRequest;

        // let mut timer = crate::timer::Timer::new();

        match req {
            WorkRequest::Call(erased) => {
                // TODO(wyj): sanity check on the addr contained in erased
                // recover the original data type based on the func_id
                match erased.meta.func_id {
                    3687134534u32 => {
                        tracing::trace!(
                            "mRPC engine got request from App, call_id={}",
                            erased.meta.call_id
                        );

                        // construct message meta on heap
                        let rpc_id = RpcId(erased.meta.conn_id, erased.meta.call_id);
                        let meta_buf_ptr = self
                            .meta_buf_pool
                            .obtain(rpc_id)
                            .expect("MessageMeta pool exhausted");
                        unsafe {
                            std::ptr::write(meta_buf_ptr.as_meta_ptr(), erased.meta);
                        }

                        let msg = RpcMessageTx {
                            meta_buf_ptr,
                            addr_backend: erased.shm_addr_backend,
                        };
                        // if access to message's data fields are desired,
                        // typed message can be conjured up here via matching func_id
                        self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                    }
                    _ => unimplemented!(),
                }
            }
            WorkRequest::Reply(erased) => {
                // recover the original data type based on the func_id
                match erased.meta.func_id {
                    3687134534u32 => {
                        tracing::trace!(
                            "mRPC engine got reply from App, call_id={}",
                            erased.meta.call_id
                        );

                        let rpc_id = RpcId(erased.meta.conn_id, erased.meta.call_id);
                        let meta_buf_ptr = self
                            .meta_buf_pool
                            .obtain(rpc_id)
                            .expect("MessageMeta pool exhausted");
                        unsafe {
                            std::ptr::write(meta_buf_ptr.as_meta_ptr(), erased.meta);
                        }

                        let msg = RpcMessageTx {
                            meta_buf_ptr,
                            addr_backend: erased.shm_addr_backend,
                        };
                        // if access to message's data
                        self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                    }
                    _ => unimplemented!(),
                }
            }
            WorkRequest::ReclaimRecvBuf(conn_id, msg_call_ids) => {
                self.tx_outputs()[0]
                    .send(EngineTxMessage::ReclaimRecvBuf(*conn_id, *msg_call_ids))?;
            }
        }

        // timer.tick();
        // log::info!("process_dp: {}", timer);
        Ok(())
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use crate::engine::graph::TryRecvError;
        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineRxMessage::RpcMessage(msg) => {
                        let meta = unsafe { *msg.meta.as_ref() };
                        tracing::trace!(
                            "mRPC engine send message to App, call_id={}",
                            meta.call_id
                        );

                        let erased = MessageErased {
                            meta,
                            shm_addr_app: msg.addr_app,
                            shm_addr_backend: msg.addr_backend,
                        };

                        // the following operation takes around 100ns
                        let mut sent = false;
                        while !sent {
                            self.customer.enqueue_wc_with(|ptr, _count| unsafe {
                                sent = true;
                                ptr.cast::<dp::Completion>().write(dp::Completion::Incoming(
                                    erased,
                                    TransportStatus::Success,
                                ));
                                1
                            })?;
                        }
                    }
                    EngineRxMessage::Ack(rpc_id, status) => {
                        // release message meta buffer
                        self.meta_buf_pool.release(rpc_id)?;
                        let mut sent = false;
                        while !sent {
                            self.customer.enqueue_wc_with(|ptr, _count| unsafe {
                                sent = true;
                                ptr.cast::<dp::Completion>()
                                    .write(dp::Completion::Outgoing(rpc_id, status));
                                1
                            })?;
                        }
                    }
                }
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn check_new_incoming_connection(&mut self) -> Result<Status, Error> {
        use ipc::mrpc::cmd::{Completion, CompletionKind};
        use tokio::sync::mpsc::error::TryRecvError;
        match self.cmd_rx.try_recv() {
            Ok(Completion(comp)) => {
                match comp {
                    Ok(CompletionKind::NewConnectionInternal(handle, recv_mrs, fds)) => {
                        // TODO(cjr): check if this send_fd will block indefinitely.
                        self.customer.send_fd(&fds).unwrap();
                        let comp_kind = CompletionKind::NewConnection((handle, recv_mrs));
                        self.customer.send_comp(cmd::Completion(Ok(comp_kind)))?;
                        Ok(Status::Progress(1))
                    }
                    other => panic!("unexpected: {:?}", other),
                }
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }
}
