use std::mem;
use std::os::unix::prelude::{AsRawFd, RawFd};
use std::sync::Arc;

use unique::Unique;

use ipc::mrpc;

use interface::engine::SchedulingMode;
use interface::rpc::{MessageTemplateErased, RpcMsgType};
use interface::AsHandle;

use super::module::ServiceType;
use super::state::{State, WrContext};
use super::ulib;
use super::{ControlPathError, DatapathError};
use crate::engine::{Engine, EngineStatus, Upgradable, Version, Vertex};
use crate::mrpc::marshal::{MessageTemplate, RpcMessage, SgList, ShmBuf, Unmarshal};
use crate::node::Node;
use crate::resource::Error as ResourceError;

pub struct TlStorage {
    pub(crate) service: ServiceType,
    pub(crate) state: State,
}

pub struct RpcAdapterEngine {
    pub(crate) tls: Box<TlStorage>,
    // shared completion queue model
    pub(crate) cq: ulib::uverbs::CompletionQueue,
    pub(crate) recent_listener_handle: Option<interface::Handle>,

    pub(crate) node: Node,
    pub(crate) cmd_rx: std::sync::mpsc::Receiver<mrpc::cmd::Command>,
    pub(crate) cmd_tx: std::sync::mpsc::Sender<mrpc::cmd::Completion>,

    pub(crate) dp_spin_cnt: usize,
    pub(crate) backoff: usize,
    pub(crate) _mode: SchedulingMode,
}

impl Upgradable for RpcAdapterEngine {
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

impl Vertex for RpcAdapterEngine {
    crate::impl_vertex_for_engine!(node);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for RpcAdapterEngine {
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>> {
        // check input queue
        self.check_input_queue()?;

        // check service
        self.check_transport_service()?;

        // TODO(cjr): check incoming connect request
        // the CmIdListener::get_request() is currently synchronous.
        // need to make it asynchronous and low cost to check.
        self.check_incoming_connection()?;

        // check input command queue
        self.check_input_cmd_queue()?;
        Ok(EngineStatus::Continue)
    }

    #[inline]
    unsafe fn tls(&self) -> Option<&'static dyn std::any::Any> {
        // let (addr, meta) = (self.tls.as_ref() as *const TlStorage).to_raw_parts();
        // let tls: *const TlStorage = std::ptr::from_raw_parts(addr, meta);
        let tls = self.tls.as_ref() as *const TlStorage;
        Some(&*tls)
    }
}

impl RpcAdapterEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        // TODO(cjr): check from local queue
        // TODO(cjr): check credit
        use std::sync::mpsc::TryRecvError;
        use ulib::uverbs::SendFlags;
        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                // get cmid from conn_id
                let msg = unsafe { msg.as_ref() };
                let cmid_handle = msg.conn_id();
                let conn_ctx = self.tls.state.resource().cmid_table.get(&cmid_handle)?;
                let cmid = &conn_ctx.cmid;
                // Sender marshals the data (gets an SgList)
                let sglist = msg.marshal();
                // Sender posts send requests from the SgList
                for (i, &sge) in sglist.0.iter().enumerate() {
                    // query mr from each sge
                    let mr = self.tls.state.resource().query_mr(sge)?;
                    let off = sge.ptr - mr.as_ptr() as usize;
                    if i + 1 < sglist.0.len() {
                        // post send
                        unsafe {
                            cmid.post_send(&mr, off..off + sge.len, 0, SendFlags::SIGNALED)?;
                        }
                    } else {
                        // post send with imm
                        unsafe {
                            cmid.post_send_with_imm(
                                &mr,
                                off..off + sge.len,
                                0,
                                SendFlags::SIGNALED,
                                0,
                            )?;
                        }
                    }
                }
                // Sender posts an extra SendWithImm
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn unmarshal_and_deliver_up(&mut self, sgl: SgList) -> Result<Status, DatapathError> {
        use crate::mrpc::codegen;
        let erased = unsafe { MessageTemplateErased::unmarshal(sgl.clone()) }.unwrap();
        let meta = unsafe { erased.as_ref() }.meta;
        let dyn_msg = match meta.msg_type {
            RpcMsgType::Request => {
                match meta.func_id {
                    0 => {
                        let msg = unsafe {
                            MessageTemplate::<codegen::HelloRequest>::unmarshal(sgl).unwrap()
                        };
                        // Safety: this is fine here because msg is already a unique
                        // pointer
                        let dyn_msg =
                            unsafe { Unique::new_unchecked(msg.as_ptr() as *mut dyn RpcMessage) };
                        dyn_msg
                    }
                    _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                }
            }
            RpcMsgType::Response => {
                match meta.func_id {
                    0 => {
                        let msg = unsafe {
                            MessageTemplate::<codegen::HelloReply>::unmarshal(sgl).unwrap()
                        };
                        // Safety: this is fine here because msg is already a unique
                        // pointer
                        let dyn_msg =
                            unsafe { Unique::new_unchecked(msg.as_ptr() as *mut dyn RpcMessage) };
                        dyn_msg
                    }
                    _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                }
            }
        };
        self.rx_outputs()[0].send(dyn_msg).unwrap();
        Ok(Progress(1))
    }

    fn check_transport_service(&mut self) -> Result<Status, DatapathError> {
        // check completion, and replenish some recv requests
        use interface::{WcFlags, WcOpcode, WcStatus};
        let mut comps = Vec::with_capacity(32);
        self.cq.poll(&mut comps)?;
        for wc in comps {
            match wc.status {
                WcStatus::Success => {
                    match wc.opcode {
                        WcOpcode::Send => {
                            // send completed, do nothing
                        }
                        WcOpcode::Recv => {
                            let wr_ctx = self.tls.state.resource().wr_contexts.get(&wc.wr_id)?;
                            let cmid_handle = wr_ctx.conn_id;
                            let mut conn_ctx =
                                self.tls.state.resource().cmid_table.get(&cmid_handle)?;
                            if !wc.wc_flags.contains(WcFlags::WITH_IMM) {
                                // received a segment of RPC message
                                let sge = ShmBuf {
                                    ptr: wr_ctx.mr_addr,
                                    len: wc.byte_len as _,
                                };
                                Arc::get_mut(&mut conn_ctx)
                                    .unwrap()
                                    .receiving_sgl
                                    .0
                                    .push(sge);
                            } else {
                                // received an entire RPC message
                                let sgl = mem::take(
                                    &mut Arc::get_mut(&mut conn_ctx).unwrap().receiving_sgl,
                                );
                                self.unmarshal_and_deliver_up(sgl)?;
                            }
                        }
                        WcOpcode::Invalid => panic!("invalid wc: {:?}", wc),
                        _ => panic!("Unhandled wc opcode: {:?}", wc),
                    }
                }
                WcStatus::Error(_) => {
                    eprintln!("wc failed: {:?}", wc);
                }
            }
        }
        // COMMENT(cjr): Progress(0) here is okay for now because we haven't use the progress as
        // any indicator.
        Ok(Status::Progress(0))
    }

    fn check_incoming_connection(&mut self) -> Result<Status, ControlPathError> {
        // TODO(cjr): should check for each connection.......... shit!
        if let Some(recent) = self.recent_listener_handle.as_ref() {
            let listener = self.tls.state.resource().listener_table.get(recent)?;
            if let Some(mut builder) = listener.try_get_request()? {
                // establish connection
                let mut pre_id = builder
                    .set_send_cq(&self.cq)
                    .set_recv_cq(&self.cq)
                    .set_max_send_wr(128)
                    .set_max_recv_wr(128)
                    .build()?;
                // prepare and post receive buffers
                let mut recv_mrs = Vec::with_capacity(128);
                let (returned_mrs, fds) = self.prepare_recv_buffers(&mut pre_id, &mut recv_mrs)?;
                // accept
                let id = pre_id.accept(None)?;
                let handle = id.as_handle();
                // insert resources after connection establishment
                self.tls.state.resource().insert_cmid(id, 128)?;
                for recv_mr in recv_mrs {
                    self.tls
                        .state
                        .resource()
                        .recv_mr_table
                        .insert(recv_mr.as_handle(), recv_mr)?;
                }
                // pass these resources back to the user
                let comp = mrpc::cmd::Completion(Ok(
                    mrpc::cmd::CompletionKind::NewConnectionInternal(handle, returned_mrs, fds),
                ));
                self.cmd_tx.send(comp)?;
            }
            Ok(Status::Progress(1))
        } else {
            Ok(Status::Progress(0))
        }
    }

    fn prepare_recv_buffers(
        &self,
        pre_id: &mut ulib::ucm::PreparedCmId,
        recv_mrs: &mut Vec<ulib::uverbs::MemoryRegion<u8>>,
    ) -> Result<(Vec<interface::returned::MemoryRegion>, Vec<RawFd>), ControlPathError> {
        // create 128 receive mrs, post recv requestse and
        // This is safe because even though recv_mr is moved, the backing memfd and
        // mapped memory regions are still there, and nothing of this mr is changed.
        // We also make sure the lifetime of the mr is longer by storing it in the
        // state.
        for _ in 0..128 {
            let recv_mr: ulib::uverbs::MemoryRegion<u8> = pre_id.alloc_msgs(8 * 1024 * 1024)?;
            recv_mrs.push(recv_mr);
        }
        for recv_mr in recv_mrs.iter_mut() {
            let wr_id = recv_mr.as_handle().0 as u64;
            let wr_ctx = WrContext {
                conn_id: pre_id.as_handle(),
                mr_addr: recv_mr.as_ptr() as usize,
            };
            unsafe {
                pre_id.post_recv(recv_mr, .., wr_id)?;
            }
            self.tls
                .state
                .resource()
                .wr_contexts
                .insert(wr_id, wr_ctx)?;
        }
        let mut returned_mrs = Vec::with_capacity(128);
        let mut fds = Vec::with_capacity(128);
        for recv_mr in recv_mrs {
            returned_mrs.push(interface::returned::MemoryRegion {
                handle: recv_mr.inner,
                rkey: recv_mr.rkey(),
                vaddr: recv_mr.as_ptr() as u64,
                pd: recv_mr.pd().inner,
            });
            fds.push(recv_mr.memfd().as_raw_fd());
        }
        Ok((returned_mrs, fds))
    }

    fn check_input_cmd_queue(&mut self) -> Result<Status, ControlPathError> {
        use std::sync::mpsc::TryRecvError;
        match self.cmd_rx.try_recv() {
            Ok(req) => {
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.cmd_tx.send(mrpc::cmd::Completion(Ok(res)))?,
                    Err(ControlPathError::InProgress) => return Ok(Progress(0)),
                    Err(ControlPathError::NoResponse) => return Ok(Progress(1)),
                    Err(e) => self.cmd_tx.send(mrpc::cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn process_cmd(
        &mut self,
        req: &mrpc::cmd::Command,
    ) -> Result<mrpc::cmd::CompletionKind, ControlPathError> {
        match req {
            mrpc::cmd::Command::SetTransport(_) => {
                unreachable!();
            }
            mrpc::cmd::Command::AllocShm(nbytes) => {
                let pd = &self.tls.state.resource().default_pds()[0];
                let access = ulib::uverbs::AccessFlags::REMOTE_READ
                    | ulib::uverbs::AccessFlags::REMOTE_WRITE
                    | ulib::uverbs::AccessFlags::LOCAL_WRITE;
                let mr: ulib::uverbs::MemoryRegion<u8> = pd.allocate(*nbytes, access)?;
                let returned_mr = interface::returned::MemoryRegion {
                    handle: mr.inner,
                    rkey: mr.rkey(),
                    vaddr: mr.as_ptr() as u64,
                    pd: mr.pd().inner,
                };
                // store the allocated MRs for later memory address translation
                let memfd = mr.memfd().as_raw_fd();
                self.tls
                    .state
                    .resource()
                    .mr_table
                    .lock()
                    .insert(mr.as_ptr() as usize, Arc::new(mr))
                    .map_or(Err(ResourceError::Exists), |_| Ok(()))?;
                Ok(mrpc::cmd::CompletionKind::AllocShmInternal(
                    returned_mr,
                    memfd,
                ))
            }
            mrpc::cmd::Command::Connect(addr) => {
                log::trace!("Connect, addr: {:?}", addr);
                // create CmIdBuilder
                let builder = ulib::ucm::CmIdBuilder::new()
                    .set_send_cq(&self.cq)
                    .set_recv_cq(&self.cq)
                    .set_max_send_wr(128)
                    .set_max_recv_wr(128)
                    .resolve_route(addr)?;
                let mut pre_id = builder.build()?;
                // prepare and post receive buffers
                let mut recv_mrs = Vec::with_capacity(128);
                let (returned_mrs, fds) = self.prepare_recv_buffers(&mut pre_id, &mut recv_mrs)?;
                // connect
                let id = pre_id.connect(None)?;
                let handle = id.as_handle();

                // insert resources after connection establishment
                self.tls.state.resource().insert_cmid(id, 128)?;
                for recv_mr in recv_mrs {
                    self.tls
                        .state
                        .resource()
                        .recv_mr_table
                        .insert(recv_mr.as_handle(), recv_mr)?;
                }
                Ok(mrpc::cmd::CompletionKind::ConnectInternal(
                    handle,
                    returned_mrs,
                    fds,
                ))
            }
            mrpc::cmd::Command::Bind(addr) => {
                log::trace!("Bind, addr: {:?}", addr);
                // create CmIdBuilder
                let listener = ulib::ucm::CmIdBuilder::new().bind(addr)?;
                let handle = listener.as_handle();
                self.tls
                    .state
                    .resource()
                    .listener_table
                    .insert(handle, listener)?;
                self.recent_listener_handle.replace(handle);
                Ok(mrpc::cmd::CompletionKind::Bind(handle))
            }
            mrpc::cmd::Command::NewMappedAddrs(app_vaddrs) => {
                // find those existing mrs, and update their app_vaddrs
                for (mr_handle, app_vaddr) in app_vaddrs {
                    let mut mr = self.tls.state.resource().recv_mr_table.get(mr_handle)?;
                    Arc::get_mut(&mut mr).unwrap().app_vaddr = *app_vaddr;
                }
                Err(ControlPathError::NoResponse)
            }
        }
    }
}
