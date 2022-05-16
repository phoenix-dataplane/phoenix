use std::future::Future;

use interface::rpc::{MessageMeta, MessageTemplateErased, RpcMsgType};

use interface::engine::SchedulingMode;
use ipc::mrpc::{cmd, control_plane, dp};
use ipc::shmalloc::ShmPtr;

use super::module::CustomerType;
use super::state::State;
use super::{DatapathError, Error};
use crate::engine::{future, Engine, EngineResult, Indicator, Vertex};
use crate::mrpc::marshal::RpcMessage;
use crate::node::Node;

pub struct MrpcEngine {
    pub(crate) _state: State,

    pub(crate) total_exec_time: i64,
    pub(crate) num_execs: i64,

    pub(crate) flag: bool,

    pub(crate) customer: CustomerType,
    pub(crate) node: Node,
    pub(crate) cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
    pub(crate) cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,

    pub(crate) _mode: SchedulingMode,

    pub(crate) transport_type: Option<control_plane::TransportType>,

    pub(crate) indicator: Option<Indicator>,
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

            let mut nwork = 0;

            let start = std::time::Instant::now();

            if let Progress(n) = self.check_customer()? {
                nwork += n;
            }

            if let Progress(n) = self.check_input_queue()? {
                nwork += n;

            }

            if self.customer.has_control_command() {
                self.flush_dp()?;
                if let Status::Disconnected = self.check_cmd().await? {
                    return Ok(());
                }
            }

            self.check_new_incoming_connection()?;

            self.indicator.as_ref().unwrap().set_nwork(nwork);


            let dur = start.elapsed().as_nanos();
            if nwork > 0 {
                self.total_exec_time += dur as i64;
                self.num_execs += 1;
            }

            self.log_exec_time();


            future::yield_now().await;
        }
    }

    fn log_exec_time(&mut self) {
        if self.num_execs % 1000 == 0 && self.num_execs > 1 {
            log::warn!("MrpcEngine main loop exec time {} ns, #execs={}", self.total_exec_time / self.num_execs, self.num_execs);
            self.total_exec_time = 0;
            self.num_execs = 0;
        }
    }
}

impl MrpcEngine {
    fn flush_dp(&mut self) -> Result<Status, DatapathError> {
        // unimplemented!();
        Ok(Status::Progress(0))
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
            // Command::AllocShm(nbytes) => {
            //     self.cmd_tx.send(Command::AllocShm(*nbytes)).unwrap();
            //     match self.cmd_rx.recv().unwrap().0 {
            //         Ok(CompletionKind::AllocShmInternal(returned_mr, memfd)) => {
            //             self.customer.send_fd(&[memfd]).unwrap();
            //             Ok(CompletionKind::AllocShm(returned_mr))
            //         }
            //         other => panic!("unexpected: {:?}", other),
            //     }
            // }
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
            }
            // Command::NewMappedAddrs(app_vaddrs) => {
            //     // just forward it
            //     self.cmd_tx
            //         .send(Command::NewMappedAddrs(app_vaddrs.clone()))
            //         .unwrap();
            //     match self.cmd_rx.recv().unwrap().0 {
            //         Ok(CompletionKind::NewMappedAddrsInternal(addr_map)) => {
            //             for tup in addr_map {
            //                 let local_addr = tup.0;
            //                 let buf = ShmBuf {
            //                     ptr: tup.1,
            //                     len: tup.2,
            //                 };
            //                 log::debug!(
            //                     "NewMappedAddrs, local: {:#0x}, app_addr: {:#0x}, len: {}",
            //                     local_addr,
            //                     buf.ptr,
            //                     buf.len
            //                 );
            //                 self.state.resource().insert_addr_map(local_addr, buf)?;
            //             }
            //             Ok(CompletionKind::NewMappedAddrs)
            //         }
            //         other => panic!("unexpected: {:?}", other),
            //     }
            //     // Err(Error::NoReponse)
            // }
        }
    }

    fn check_customer(&mut self) -> Result<Status, DatapathError> {
        use dp::WorkRequest;
        const BUF_LEN: usize = 32;

        // Fetch available work requests. Copy them into a buffer.
        let max_count = BUF_LEN.min(self.customer.get_avail_wc_slots()?);
        if max_count == 0 {
            return Ok(Progress(0));
        }

        let mut count = 0;
        let mut buffer = Vec::with_capacity(BUF_LEN);

        self.customer
            .dequeue_wr_with(|ptr, read_count| unsafe {
                debug_assert!(max_count <= BUF_LEN);
                count = max_count.min(read_count);
                for i in 0..count {
                    buffer.push(ptr.add(i).cast::<WorkRequest>().read());
                }
                count
            })
            .unwrap_or_else(|e| panic!("check_customer: {}", e));

        // Process the work requests.

        for wr in &buffer {
            self.process_dp(wr)?;
        }

        Ok(Progress(count))
    }

    fn process_dp(&mut self, req: &dp::WorkRequest) -> Result<(), DatapathError> {
        use crate::mrpc::codegen;
        use crate::mrpc::marshal::MessageTemplate;
        use dp::WorkRequest;
        log::info!("mRPC engine got request/reply from App");
        match req {
            WorkRequest::Call(erased) => {
                // TODO(wyj): sanity check on the addr contained in erased
                // recover the original data type based on the func_id
                match erased.meta.func_id {
                    0 => {
                        let mut msg =
                            unsafe { MessageTemplate::<codegen::HelloRequest>::new(*erased) };
                        // Safety: this is fine here because msg is already a unique
                        // pointer
                        log::debug!("start to marshal");
                        unsafe { msg.as_ref() }.marshal();
                        // MessageTemplate::<codegen::HelloRequest>::marshal(unsafe { msg.as_ref() });
                        log::debug!("end marshal");
                        let dyn_msg = 
                            unsafe { ShmPtr::new(msg.as_mut() as *mut dyn RpcMessage, msg.get_remote_addr()).unwrap() };
                        self.tx_outputs()[0].send(dyn_msg)?;
                    }
                    _ => unimplemented!(),
                }
            }
            WorkRequest::Reply(erased) => {
                // recover the original data type based on the func_id
                match erased.meta.func_id {
                    0 => {
                        let mut msg =
                            unsafe { MessageTemplate::<codegen::HelloReply>::new(*erased) };
                        // Safety: this is fine here because msg is already a unique
                        // pointer
                        let dyn_msg = 
                            unsafe { ShmPtr::new(msg.as_mut() as *mut dyn RpcMessage, msg.get_remote_addr()).unwrap() };
                        self.tx_outputs()[0].send(dyn_msg)?;
                    }
                    _ => unimplemented!(),
                }
            }
        }
        Ok(())
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use tokio::sync::mpsc::error::TryRecvError;
        match self.rx_inputs()[0].try_recv() {
            Ok(mut msg) => {
                // deliver the msg to application
                let msg_ref = unsafe { msg.as_ref() };
                let meta = MessageMeta {
                    conn_id: msg_ref.conn_id(),
                    func_id: msg_ref.func_id(),
                    call_id: msg_ref.call_id(),
                    len: msg_ref.len(),
                    msg_type: if msg_ref.is_request() {
                        RpcMsgType::Request
                    } else {
                        RpcMsgType::Response
                    },
                };
                // TODO(cjr): switch_address_space
                // msg.switch_address_space();
                let msg_mut = unsafe { msg.as_mut() };
                msg_mut.switch_address_space();
                // NOTE(wyj): this is the address of the pointer to `msg_mut`
                // while switch_address_space swithces the address of the actual payload
                // TODO(wyj): check which should be shm_addr, which should be shm_addr_remote
                // NOTE(wyj): This is message from backend to app
                // MessageTemplateErased is a just a message header
                // it does not holder actual payload
                // but a pointer to the payload on shared memory
                let erased = MessageTemplateErased {
                    meta,
                    // casting to thin pointer first, drop the Pointee::Metadata
                    shm_addr: msg.get_remote_addr(),
                    shm_addr_remote: msg.as_ptr() as *const () as usize,
                    // shmptr: msg.as_ptr() as *mut MessageTemplateErased as u64,
                };
                log::info!("mRPC engine send message to App");
                let mut sent = false;
                while !sent {
                    self.customer.enqueue_wc_with(|ptr, _count| unsafe {
                        sent = true;
                        ptr.cast::<dp::Completion>()
                            .write(dp::Completion { erased });
                        1
                    })?;
                }
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn check_new_incoming_connection(&mut self) -> Result<Status, Error> {
        if self.flag {return Ok(Progress(0)) }
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
                        self.flag = true;
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
