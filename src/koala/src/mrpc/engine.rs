use std::time::{Duration, Instant};

use interface::rpc::{MessageMeta, MessageTemplateErased, RpcMsgType};
use unique::Unique;

use interface::engine::SchedulingMode;
use ipc::mrpc::{cmd, control_plane, dp};

use super::module::CustomerType;
use super::{DatapathError, Error};
use crate::engine::{Engine, EngineStatus, Upgradable, Version, Vertex};
use crate::mrpc::marshal::RpcMessage;
use crate::node::Node;

pub struct MrpcEngine {
    pub(crate) customer: CustomerType,
    pub(crate) node: Node,
    pub(crate) cmd_tx: std::sync::mpsc::Sender<cmd::Command>,
    pub(crate) cmd_rx: std::sync::mpsc::Receiver<cmd::Completion>,

    pub(crate) dp_spin_cnt: usize,
    pub(crate) backoff: usize,
    pub(crate) _mode: SchedulingMode,

    // state
    pub(crate) transport_type: Option<control_plane::TransportType>,

    // otherwise, the
    pub(crate) last_cmd_ts: Instant,
}

impl Upgradable for MrpcEngine {
    fn version(&self) -> Version {
        unimplemented!();
    }

    fn check_compatible(&self, _v2: Version) -> bool {
        unimplemented!();
    }

    fn suspend(&mut self) {
        unimplemented!();
    }

    fn dump(&self) {
        unimplemented!();
    }

    fn restore(&mut self) {
        unimplemented!();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Vertex for MrpcEngine {
    crate::impl_vertex_for_engine!(node);
}

impl Engine for MrpcEngine {
    fn description(&self) -> String {
        format!("MrpcEngine, todo show more information")
    }

    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>> {
        const DP_LIMIT: usize = 1 << 17;
        const CMD_MAX_INTERVAL_MS: u64 = 1000;
        if let Progress(n) = self.check_customer()? {
            if n > 0 {
                self.backoff = DP_LIMIT.min(self.backoff * 2);
            }
        }

        self.check_input_queue()?;

        self.dp_spin_cnt += 1;
        if self.dp_spin_cnt < self.backoff {
            return Ok(EngineStatus::Continue);
        }

        self.dp_spin_cnt = 0;

        if self.customer.has_control_command()
            || self.last_cmd_ts.elapsed() > Duration::from_millis(CMD_MAX_INTERVAL_MS)
        {
            self.last_cmd_ts = Instant::now();
            self.backoff = std::cmp::max(1, self.backoff / 2);
            self.flush_dp()?;
            if let Status::Disconnected = self.check_cmd()? {
                return Ok(EngineStatus::Complete);
            }
        } else {
            self.backoff = DP_LIMIT.min(self.backoff * 2);
        }

        self.check_new_incoming_connection()?;

        Ok(EngineStatus::Continue)
    }
}

impl MrpcEngine {
    fn flush_dp(&mut self) -> Result<Status, DatapathError> {
        // unimplemented!();
        Ok(Status::Progress(0))
    }

    fn check_cmd(&mut self) -> Result<Status, Error> {
        match self.customer.try_recv_cmd() {
            // handle request
            Ok(req) => {
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Err(Error::NoReponse) => {} // no need to do anything
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

    fn process_cmd(&mut self, req: &cmd::Command) -> Result<cmd::CompletionKind, Error> {
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
            Command::AllocShm(nbytes) => {
                self.cmd_tx.send(Command::AllocShm(*nbytes)).unwrap();
                match self.cmd_rx.recv().unwrap().0 {
                    Ok(CompletionKind::AllocShmInternal(returned_mr, memfd)) => {
                        self.customer.send_fd(&[memfd])?;
                        Ok(CompletionKind::AllocShm(returned_mr))
                    }
                    other => panic!("unexpected: {:?}", other),
                }
            }
            Command::Connect(addr) => {
                self.cmd_tx.send(Command::Connect(*addr)).unwrap();
                match self.cmd_rx.recv().unwrap().0 {
                    Ok(CompletionKind::ConnectInternal(handle, recv_mrs, fds)) => {
                        self.customer.send_fd(&fds)?;
                        Ok(CompletionKind::Connect((handle, recv_mrs)))
                    }
                    other => panic!("unexpected: {:?}", other),
                }
            }
            Command::Bind(addr) => {
                self.cmd_tx.send(Command::Bind(*addr)).unwrap();
                match self.cmd_rx.recv().unwrap().0 {
                    Ok(CompletionKind::Bind(listener_handle)) => {
                        // just forward it
                        Ok(CompletionKind::Bind(listener_handle))
                    }
                    other => panic!("unexpected: {:?}", other),
                }
            }
            cmd::Command::NewMappedAddrs(app_vaddrs) => {
                // just forward it
                self.cmd_tx.send(cmd::Command::NewMappedAddrs(app_vaddrs.clone())).unwrap();
                Err(Error::NoReponse)
            }
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

        Ok(Progress(0))
    }

    fn process_dp(&mut self, req: &dp::WorkRequest) -> Result<(), DatapathError> {
        use crate::mrpc::codegen;
        use crate::mrpc::marshal::MessageTemplate;
        use dp::WorkRequest;
        match req {
            WorkRequest::Call(erased) => {
                // recover the original data type based on the func_id
                match erased.meta.func_id {
                    0 => {
                        let mut msg = unsafe { MessageTemplate::<codegen::HelloRequest>::new(*erased) };
                        // Safety: this is fine here because msg is already a unique
                        // pointer
                        log::debug!("start to marshal");
                        unsafe { msg.as_ref() }.marshal();
                        // MessageTemplate::<codegen::HelloRequest>::marshal(unsafe { msg.as_ref() });
                        log::debug!("end marshal");
                        let dyn_msg =
                            unsafe { Unique::new(msg.as_mut() as *mut dyn RpcMessage).unwrap() };
                        self.tx_outputs()[0].send(dyn_msg)?;
                    }
                    _ => unimplemented!(),
                }
            }
            WorkRequest::Reply(erased) => {
                // recover the original data type based on the func_id
                match erased.meta.func_id {
                    0 => {
                        let mut msg = unsafe { MessageTemplate::<codegen::HelloReply>::new(*erased) };
                        // Safety: this is fine here because msg is already a unique
                        // pointer
                        let dyn_msg =
                            unsafe { Unique::new(msg.as_mut() as *mut dyn RpcMessage).unwrap() };
                        self.tx_outputs()[0].send(dyn_msg)?;
                    }
                    _ => unimplemented!(),
                }
            }
        }
        Ok(())
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use std::sync::mpsc::TryRecvError;
        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                // deliver the msg to application
                // TODO(cjr): switch_address_space
                // msg.switch_address_space();
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
                let erased = MessageTemplateErased {
                    meta,
                    // casting to thin pointer first, drop the Pointee::Metadata
                    shmptr: msg.as_ptr() as *mut MessageTemplateErased as u64,
                };
                let mut sent = false;
                while !sent {
                    self.customer.enqueue_wc_with(|ptr, _count| unsafe {
                        sent = true;
                        ptr.cast::<dp::Completion>().write(dp::Completion { erased });
                        1
                    })?;
                }
                Ok(Progress(0))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn check_new_incoming_connection(&mut self) -> Result<Status, Error> {
        use ipc::mrpc::cmd::{CompletionKind, Completion};
        use std::sync::mpsc::TryRecvError;
        match self.cmd_rx.try_recv() {
            Ok(Completion(comp)) => {
                match comp {
                    Ok(CompletionKind::NewConnectionInternal(handle, recv_mrs, fds)) => {
                        // TODO(cjr): check if this send_fd will block indefinitely.
                        self.customer.send_fd(&fds)?;
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
