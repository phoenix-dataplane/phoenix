use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::ops::Range;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;

use interface::Handle;
use ipc::{self, cmd, dp};

use engine::{Engine, SchedulingMode, Upgradable, Version};

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

/// TODO(cjr): replace this later.
const ENGINE_PATH: &str = "/tmp/koala/koala-tranport-engine.sock";

/// A variety of tables map a `Handle` to a kind of RNIC resource.
#[derive(Default)]
struct Resource<'ctx> {
    handle_cnt: usize,
    pd_table: HashMap<Handle, ibv::ProtectionDomain<'ctx>>,
    cq_table: HashMap<Handle, ibv::CompletionQueue<'ctx>>,
    cmid_table: HashMap<Handle, CmId>,
    mr_table: HashMap<Handle, rdmacm::MemoryRegion>,
}

/// This table should be shared between multiple transport engines, it must allow concurrent access
/// and modification. But for now, we implement it without considering synchronization.
#[derive(Debug)]
struct MemoryTranslationTable {
    // mr_handle -> SharedMemoryFile
    table: HashMap<Handle, SharedMemoryFile>,
}

/// A piece of memory region that is both mmap shared and registered in the NIC.
#[derive(Debug)]
struct SharedMemoryFile {
    urange: Range<u64>,
    kbuf: &'static [u8],
    memfile: File,
    path: PathBuf,
}

impl SharedMemoryFile {
    fn new<P: AsRef<Path>>(urange: Range<u64>, buffer: &[u8], memfile: File, path: P) -> Self {
        SharedMemoryFile {
            urange,
            kbuf: unsafe { slice::from_raw_parts(buffer.as_ptr(), buffer.len()) },
            memfile,
            path: path.as_ref().to_owned(),
        }
    }
}

impl MemoryTranslationTable {
    fn new() -> Self {
        MemoryTranslationTable {
            table: Default::default(),
        }
    }
    fn allocate(&mut self, mr_handle: Handle, smf: SharedMemoryFile) {
        self.table.insert(mr_handle, smf).ok_or(()).unwrap_err();
    }
}

impl<'ctx> Resource<'ctx> {
    fn new() -> Self {
        Default::default()
    }

    fn allocate_handle(&mut self) -> Handle {
        self.handle_cnt += 1;
        Handle(self.handle_cnt)
    }
}

pub struct TransportEngine<'ctx> {
    /// This is the path of the domain socket which is client side is listening on.
    /// The mainly purpose of keeping is to send file descriptors to the client.
    client_path: PathBuf,
    sock: UnixDatagram,
    cmd_tx: ipc::Sender<cmd::Response>,
    cmd_rx: ipc::Receiver<cmd::Request>,
    dp_tx: ipc::Sender<dp::Response>,
    dp_rx: ipc::Receiver<dp::Request>,
    mode: SchedulingMode,

    resource: Resource<'ctx>,
    mtt: MemoryTranslationTable,
}

impl<'ctx> TransportEngine<'ctx> {
    pub fn new<P: AsRef<Path>>(
        client_path: P,
        cmd_tx: ipc::Sender<cmd::Response>,
        cmd_rx: ipc::Receiver<cmd::Request>,
        dp_tx: ipc::Sender<dp::Response>,
        dp_rx: ipc::Receiver<dp::Request>,
        mode: SchedulingMode,
    ) -> Self {
        let engine_path = PathBuf::from(ENGINE_PATH.to_string());
        if engine_path.exists() {
            fs::remove_file(&engine_path).expect("remove_file");
        }
        let sock = UnixDatagram::bind(&engine_path).expect("create unix domain socket failed");
        TransportEngine {
            client_path: client_path.as_ref().to_owned(),
            sock,
            cmd_tx,
            cmd_rx,
            dp_tx,
            dp_rx,
            mode,
            resource: Resource::new(),
            mtt: MemoryTranslationTable::new(), // it should be passed in from the transport module
        }
    }
}

impl<'ctx> Upgradable for TransportEngine<'ctx> {
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

    fn resume(&mut self) {
        unimplemented!();
    }
}

impl<'ctx> Engine for TransportEngine<'ctx> {
    fn init(&mut self) {
        unimplemented!();
    }

    fn run(&mut self) -> bool {
        if self.check_dp() {
            return true;
        }
        if self.check_cmd() {
            return true;
        }
        false
    }

    fn shutdown(&mut self) {
        unimplemented!();
    }

    fn enqueue(&self) {
        unimplemented!();
    }

    fn check_queue_len(&self) {
        unimplemented!();
    }
}

impl<'ctx> TransportEngine<'ctx> {
    fn check_dp(&mut self) -> bool {
        match self.dp_rx.try_recv() {
            // handle request
            Ok(req) => {
                let res = self.process_dp(req);
                self.dp_tx.send(res).map_or_else(|_| true, |_| false)
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                false
            }
            Err(ipc::TryRecvError::IpcError(e)) => {
                if matches!(e, ipc::IpcError::Disconnected) {
                    return true;
                }
                panic!("recv error: {:?}", e);
            }
        }
    }

    fn check_cmd(&mut self) -> bool {
        match self.cmd_rx.try_recv() {
            // handle request
            Ok(req) => {
                let res = self.process_cmd(req);
                self.cmd_tx.send(res).map_or_else(|_| true, |_| false)
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                false
            }
            Err(ipc::TryRecvError::IpcError(e)) => {
                if matches!(e, ipc::IpcError::Disconnected) {
                    return true;
                }
                panic!("recv error: {:?}", e);
            }
        }
    }

    /// Process data path operations.
    fn process_dp(&mut self, req: dp::Request) -> dp::Response {
        use ipc::dp::{Request, Response};
        match req {
            Request::PostRecv(cmid_handle, wr_id, addr_range, mr_handle) => {
                let ret = self
                    .resource
                    .cmid_table
                    .get(&cmid_handle)
                    .ok_or(interface::Error::NotFound);
                if let Err(e) = ret {
                    return Response::PostRecv(Err(e));
                }
                let cmid = ret.unwrap();
                let ret = self
                    .resource
                    .mr_table
                    .get(&mr_handle)
                    .ok_or(interface::Error::NotFound);
                if let Err(e) = ret {
                    return Response::PostRecv(Err(e));
                }
                let mr = ret.unwrap();
                let ret = self
                    .mtt
                    .table
                    .get(&mr_handle)
                    .ok_or(interface::Error::NotFound);
                if let Err(e) = ret {
                    return Response::PostRecv(Err(e));
                }
                let smf = ret.unwrap();
                // TODO(cjr): shouldn't assert, should return an error
                assert!(smf.urange.start <= addr_range.start && smf.urange.end >= addr_range.end);
                let offset = (addr_range.start - smf.urange.start) as usize;
                let len = (addr_range.end - addr_range.start) as usize;
                let buf = &smf.kbuf[offset..len];
                let ret = unsafe { cmid.post_recv(wr_id, buf, &mr) }
                    .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()));
                Response::PostRecv(ret)
            }
            Request::PostSend(cmid_handle, wr_id, addr_range, mr_handle, send_flags) => {
                // TODO(cjr): send_flags is currently ignored
                let ret = self
                    .resource
                    .cmid_table
                    .get(&cmid_handle)
                    .ok_or(interface::Error::NotFound);
                if let Err(e) = ret {
                    return Response::PostSend(Err(e));
                }
                let cmid = ret.unwrap();
                let ret = self
                    .resource
                    .mr_table
                    .get(&mr_handle)
                    .ok_or(interface::Error::NotFound);
                if let Err(e) = ret {
                    return Response::PostSend(Err(e));
                }
                let mr = ret.unwrap();
                let ret = self
                    .mtt
                    .table
                    .get(&mr_handle)
                    .ok_or(interface::Error::NotFound);
                if let Err(e) = ret {
                    return Response::PostRecv(Err(e));
                }
                let smf = ret.unwrap();
                // TODO(cjr): shouldn't assert, should return an error
                assert!(smf.urange.start <= addr_range.start && smf.urange.end >= addr_range.end);
                let offset = (addr_range.start - smf.urange.start) as usize;
                let len = (addr_range.end - addr_range.start) as usize;
                let buf = &smf.kbuf[offset..len];
                let ret = unsafe { cmid.post_send(wr_id, buf, &mr) }
                    .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()));
                Response::PostSend(ret)
            }
            Request::GetRecvComp(cmid_handle) => {
                let ret = self
                    .resource
                    .cmid_table
                    .get(&cmid_handle)
                    .ok_or(interface::Error::NotFound);
                if let Err(e) = ret {
                    return Response::GetRecvComp(Err(e));
                }
                let cmid = ret.unwrap();
                let ret = cmid
                    .get_recv_comp()
                    .map(|wc| wc.into())
                    .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()));
                Response::GetRecvComp(ret)
            }
            Request::GetSendComp(cmid_handle) => {
                let ret = self
                    .resource
                    .cmid_table
                    .get(&cmid_handle)
                    .ok_or(interface::Error::NotFound);
                if let Err(e) = ret {
                    return Response::GetSendComp(Err(e));
                }
                let cmid = ret.unwrap();
                let ret = cmid
                    .get_send_comp()
                    .map(|wc| wc.into())
                    .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()));
                Response::GetSendComp(ret)
            }
        }
    }

    /// Process control path operations.
    fn process_cmd(&mut self, req: cmd::Request) -> cmd::Response {
        use ipc::cmd::{Request, Response};
        match req {
            Request::NewClient(..) => unreachable!(),
            Request::Hello(number) => Response::HelloBack(number),
            Request::GetAddrInfo(node, service, hints) => {
                trace!(
                    "node: {:?}, service: {:?}, hints: {:?}",
                    node,
                    service,
                    hints,
                );
                let hints = hints.map(rdmacm::AddrInfoHints::from);
                let ai = rdmacm::AddrInfo::getaddrinfo(
                    node.as_deref(),
                    service.as_deref(),
                    hints.as_ref(),
                );
                let ret = ai.map_or_else(
                    |e| Err(interface::Error::GetAddrInfo(e.raw_os_error().unwrap())),
                    |ai| Ok(ai.into()),
                );
                Response::GetAddrInfo(ret)
            }
            Request::CreateEp(ai, pd_handle, qp_init_attr) => {
                trace!(
                    "ai: {:?}, pd_handle: {:?}, qp_init_attr: {:?}",
                    ai,
                    pd_handle,
                    qp_init_attr
                );

                let pd = pd_handle.and_then(|h| self.resource.pd_table.get(&h));
                let qp_init_attr = qp_init_attr.map(|a| {
                    let attr = ibv::QpInitAttr {
                        qp_context: 0,
                        send_cq: a.send_cq.and_then(|h| self.resource.cq_table.get(&h.0)),
                        recv_cq: a.recv_cq.and_then(|h| self.resource.cq_table.get(&h.0)),
                        cap: a.cap.into(),
                        qp_type: a.qp_type.into(),
                        sq_sig_all: a.sq_sig_all,
                    };
                    attr.to_ibv_qp_init_attr()
                });

                let ret = match CmId::create_ep(&ai.into(), pd, qp_init_attr.as_ref()) {
                    Ok(cmid) => {
                        let cmid_handle = self.resource.allocate_handle();
                        self.resource
                            .cmid_table
                            .insert(cmid_handle, cmid)
                            .ok_or(())
                            .unwrap_err();
                        Ok(cmid_handle)
                    }
                    Err(e) => Err(interface::Error::RdmaCm(e.raw_os_error().unwrap())),
                };

                Response::CreateEp(ret)
            }
            Request::Listen(cmid_handle, backlog) => {
                trace!("cmid_handle: {:?}, backlog: {}", cmid_handle, backlog);
                let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                    Err(interface::Error::NotFound),
                    |listener| {
                        listener
                            .listen(backlog)
                            .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()))
                    },
                );
                Response::Listen(ret)
            }
            Request::GetRequest(cmid_handle) => {
                trace!("cmid_handle: {:?}", cmid_handle);

                let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                    Err(interface::Error::NotFound),
                    |listener| {
                        listener
                            .get_request()
                            .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()))
                    },
                );
                let ret = ret.map(|new_cmid| {
                    let new_handle = self.resource.allocate_handle();
                    self.resource
                        .cmid_table
                        .insert(new_handle, new_cmid)
                        .ok_or(())
                        .unwrap_err();
                    new_handle
                });
                Response::GetRequest(ret)
            }
            Request::Accept(cmid_handle, conn_param) => {
                trace!(
                    "cmid_handle: {:?}, conn_param: {:?}",
                    cmid_handle,
                    conn_param
                );
                warn!("TODO: conn_param is ignored for now");
                let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                    Err(interface::Error::NotFound),
                    |listener| {
                        listener
                            .accept()
                            .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()))
                    },
                );
                Response::Accept(ret)
            }
            Request::Connect(cmid_handle, conn_param) => {
                trace!(
                    "cmid_handle: {:?}, conn_param: {:?}",
                    cmid_handle,
                    conn_param
                );
                warn!("TODO: conn_param is ignored for now");
                let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                    Err(interface::Error::NotFound),
                    |cmid| {
                        cmid.connect()
                            .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()))
                    },
                );
                Response::Connect(ret)
            }
            Request::RegMsgs(cmid_handle, addr_range) => {
                trace!(
                    "cmid_handle: {:?}, addr_range: {:#x?}",
                    cmid_handle,
                    addr_range
                );
                let res = self.resource.cmid_table.get(&cmid_handle);
                if res.is_none() {
                    return Response::RegMsgs(Err(interface::Error::NotFound));
                }
                let cmid = res.unwrap();
                // 1. create/open shared memory file
                use nix::fcntl::OFlag;
                use nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, MapFlags, ProtFlags};
                use nix::sys::stat::Mode;
                use uuid::Uuid;
                // just randomly pick an string and use that for now
                let shm_path = PathBuf::from(format!("koala-{}", Uuid::new_v4())); // no /dev/shm prefix is needed
                let fd = shm_open(
                    &shm_path,
                    OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
                    Mode::S_IRUSR | Mode::S_IWUSR,
                )
                .expect("shm_open");
                // 2. mmap the user's vaddr into koala's address space through
                // __linear__ mapping (this is necessary) (actually, maybe not)
                let uaddr = addr_range.start;
                let ulen = (addr_range.end - addr_range.start) as usize;
                let page_size = 4096;
                let aligned_end = (uaddr as usize + ulen + page_size - 1) / page_size * page_size;
                let aligned_begin = uaddr as usize - uaddr as usize % page_size;
                let aligned_len = aligned_end - aligned_begin;
                // ftruncate the file
                let memfile = unsafe { File::from_raw_fd(fd) };
                memfile.set_len(aligned_len as _).expect("memfile set_len");
                let kaddr = unsafe {
                    mmap(
                        ptr::null_mut(),
                        aligned_len,
                        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                        MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
                        fd,
                        0,
                    )
                    .expect("mmap")
                };
                // 3. send fd back
                ipc::send_fd(&self.sock, &self.client_path, fd).unwrap();
                // 4. register kaddr with ibv_reg_mr
                let buf = unsafe { slice::from_raw_parts(kaddr as *const u8, aligned_len) };
                let ret = cmid
                    .reg_msgs(&buf)
                    .map_err(|e| interface::Error::RdmaCm(e.raw_os_error().unwrap()));
                let ret = ret.map(|mr| {
                    // 5. allocate mr handle
                    let new_mr = self.resource.allocate_handle();
                    self.resource
                        .mr_table
                        .insert(new_mr, mr)
                        .ok_or(())
                        .unwrap_err();
                    // 6. allocate entry in MTT (mr -> SharedMemoryFile)
                    let smf = SharedMemoryFile::new(addr_range, buf, memfile, &shm_path);
                    self.mtt.allocate(new_mr, smf);
                    new_mr
                });
                // 7. send mr handle back
                Response::RegMsgs(ret)
            }
            _ => {
                unimplemented!()
            }
        }
    }
}
