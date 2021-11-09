use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io;
use std::marker::PhantomData;
use std::ops::Range;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;

use uuid::Uuid;

use interface::returned;
use interface::Handle;
use ipc::{self, cmd, dp};

use engine::{Engine, SchedulingMode, Upgradable, Version};

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use crate::transport::Error;

/// A variety of tables map a `Handle` to a kind of RNIC resource.
#[derive(Default)]
struct Resource<'ctx> {
    cmid_cnt: usize,
    pd_table: ResourceTable<ibv::ProtectionDomain<'ctx>>,
    cq_table: ResourceTable<ibv::CompletionQueue<'ctx>>,
    qp_table: ResourceTable<ibv::QueuePair<'ctx>>,
    cmid_table: ResourceTable<CmId>,
    mr_table: ResourceTable<rdmacm::MemoryRegion>,
}

#[derive(Debug)]
struct ResourceTable<R> {
    table: HashMap<Handle, R>,
}

impl<R> Default for ResourceTable<R> {
    fn default() -> Self {
        ResourceTable {
            table: HashMap::new(),
        }
    }
}

impl<R> ResourceTable<R> {
    fn insert(&mut self, h: Handle, r: R) -> Result<(), Error> {
        match self.table.insert(h, r) {
            Some(_) => Err(Error::Exists),
            None => Ok(()),
        }
    }
    fn get(&self, h: &Handle) -> Result<&R, Error> {
        self.table.get(h).ok_or(Error::NotFound)
    }
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
    kbuf: &'static mut [u8],
    memfile: File,
    path: PathBuf,
}

impl SharedMemoryFile {
    fn new<P: AsRef<Path>>(urange: Range<u64>, buffer: &mut [u8], memfile: File, path: P) -> Self {
        SharedMemoryFile {
            urange,
            kbuf: unsafe { slice::from_raw_parts_mut(buffer.as_mut_ptr(), buffer.len()) },
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

    fn allocate_new_cmid_handle(&mut self) -> Handle {
        let ret = Handle(self.cmid_cnt);
        self.cmid_cnt += 1;
        ret
    }

    fn insert_cmid(
        &mut self,
        cmid: CmId,
    ) -> Result<(Handle, Option<(Handle, Handle, Handle)>), Error> {
        let cmid_handle = self.allocate_new_cmid_handle();
        if let Some(qp) = cmid.qp() {
            let send_cq = qp.send_cq();
            let recv_cq = qp.recv_cq();
            let scq_handle = send_cq.handle().into();
            let rcq_handle = recv_cq.handle().into();
            let qp_handle = qp.handle().into();
            self.cmid_table.insert(cmid_handle, cmid)?;
            self.qp_table.insert(qp_handle, qp)?;
            // safely ignore the error if the cq has already been created.
            let _ = self.cq_table.insert(scq_handle, send_cq);
            let _ = self.cq_table.insert(rcq_handle, recv_cq);
            let handles = Some((qp_handle, scq_handle, rcq_handle));
            Ok((cmid_handle, handles))
        } else {
            // passive cmid, no QP associated.
            self.cmid_table.insert(cmid_handle, cmid)?;
            Ok((cmid_handle, None))
        }
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
        let uuid = Uuid::new_v4();
        let sock_path = PathBuf::from(format!("/tmp/koala/koala-transport-engine-{}.sock", uuid));

        if sock_path.exists() {
            // This is impossible using uuid.
            fs::remove_file(&sock_path).expect("remove_file");
        }
        let sock = UnixDatagram::bind(&sock_path).expect("create unix domain socket failed");
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
        use dp::ResponseKind;
        match self.dp_rx.try_recv() {
            // handle request
            Ok(req) => {
                let result = self.process_dp(&req);
                match result {
                    Ok(ResponseKind::PostSend) | Ok(ResponseKind::PostRecv) => Ok(()),
                    Ok(res) => self.dp_tx.send(dp::Response(Ok(res))),
                    Err(e) => self.dp_tx.send(dp::Response(Err(e.into()))),
                }
                .map_or_else(|_| true, |_| false)
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
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.cmd_tx.send(cmd::Response(Ok(res))),
                    Err(e) => self.cmd_tx.send(cmd::Response(Err(e.into()))),
                }
                .map_or_else(|_| true, |_| false)
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
    fn process_dp(&mut self, req: &dp::Request) -> Result<dp::ResponseKind, Error> {
        use ipc::dp::{Request, ResponseKind};
        match req {
            Request::PostRecv(cmid_handle, wr_id, addr_range, mr_handle) => {
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                let mr = self.resource.mr_table.get(mr_handle)?;
                let smf = self.mtt.table.get_mut(mr_handle).ok_or(Error::NotFound)?;
                // TODO(cjr): shouldn't assert, should return an error
                assert!(smf.urange.start <= addr_range.start && smf.urange.end >= addr_range.end);
                let offset = (addr_range.start - smf.urange.start) as usize;
                let len = (addr_range.end - addr_range.start) as usize;
                let buf = &mut smf.kbuf[offset..offset + len];
                let ret = unsafe { cmid.post_recv(*wr_id, buf, mr) }.map_err(Error::RdmaCm);
                Ok(ResponseKind::PostRecv)
            }
            Request::PostSend(cmid_handle, wr_id, addr_range, mr_handle, send_flags) => {
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                let mr = self.resource.mr_table.get(mr_handle)?;
                let smf = self.mtt.table.get_mut(mr_handle).ok_or(Error::NotFound)?;
                // TODO(cjr): shouldn't assert, should return an error
                assert!(smf.urange.start <= addr_range.start && smf.urange.end >= addr_range.end);
                let offset = (addr_range.start - smf.urange.start) as usize;
                let len = (addr_range.end - addr_range.start) as usize;
                let buf = &smf.kbuf[offset..offset + len];
                let flags: ibv::SendFlags = (*send_flags).into();
                let ret =
                    unsafe { cmid.post_send(*wr_id, buf, mr, flags.0) }.map_err(Error::RdmaCm);
                Ok(ResponseKind::PostSend)
            }
            Request::GetRecvComp(cmid_handle) => {
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                let wc = cmid
                    .get_recv_comp()
                    .map(|wc| wc.into())
                    .map_err(Error::RdmaCm)?;
                Ok(ResponseKind::GetRecvComp(wc))
            }
            Request::GetSendComp(cmid_handle) => {
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                let wc = cmid
                    .get_send_comp()
                    .map(|wc| wc.into())
                    .map_err(Error::RdmaCm)?;
                Ok(ResponseKind::GetSendComp(wc))
            }
            Request::PollCq(cq, num_entries) => {
                let cq = self.resource.cq_table.get(&cq.0)?;
                // allocate num_entries here
                let mut wc = Vec::with_capacity(*num_entries);
                unsafe {
                    wc.set_len(*num_entries);
                }
                let completions = cq
                    .poll(&mut wc)
                    .map_err(|_| Error::Ibv(io::Error::last_os_error()))?;
                let ret = completions.iter().map(|x| (*x).into()).collect();
                Ok(ResponseKind::PollCq(ret))
            }
        }
    }

    /// Process control path operations.
    fn process_cmd(&mut self, req: &cmd::Request) -> Result<cmd::ResponseKind, Error> {
        use ipc::cmd::{Request, ResponseKind};
        match req {
            Request::NewClient(..) => unreachable!(),
            &Request::Hello(number) => Ok(ResponseKind::HelloBack(number)),
            Request::GetAddrInfo(node, service, hints) => {
                trace!(
                    "node: {:?}, service: {:?}, hints: {:?}",
                    node,
                    service,
                    hints,
                );
                let hints = hints.map(rdmacm::AddrInfoHints::from);
                let ret = rdmacm::AddrInfo::getaddrinfo(
                    node.as_deref(),
                    service.as_deref(),
                    hints.as_ref(),
                );
                match ret {
                    Ok(ai) => Ok(ResponseKind::GetAddrInfo(ai.into())),
                    Err(e) => Err(Error::GetAddrInfo(e)),
                }
            }
            Request::CreateEp(ai, pd_handle, qp_init_attr) => {
                trace!(
                    "ai: {:?}, pd_handle: {:?}, qp_init_attr: {:?}",
                    ai,
                    pd_handle,
                    qp_init_attr
                );

                let pd = if let Some(h) = pd_handle {
                    Some(self.resource.pd_table.get(h)?)
                } else {
                    None
                };
                let qp_init_attr = if let Some(a) = qp_init_attr {
                    let send_cq = if let Some(ref h) = a.send_cq {
                        Some(self.resource.cq_table.get(&h.0)?)
                    } else {
                        None
                    };
                    let recv_cq = if let Some(ref h) = a.recv_cq {
                        Some(self.resource.cq_table.get(&h.0)?)
                    } else {
                        None
                    };
                    let attr = ibv::QpInitAttr {
                        qp_context: 0,
                        send_cq: send_cq,
                        recv_cq: recv_cq,
                        cap: a.cap.clone().into(),
                        qp_type: a.qp_type.into(),
                        sq_sig_all: a.sq_sig_all,
                    };
                    Some(attr.to_ibv_qp_init_attr())
                } else {
                    None
                };

                match CmId::create_ep(&ai.clone().into(), pd, qp_init_attr.as_ref()) {
                    Ok(cmid) => {
                        let (cmid_handle, handles) = self.resource.insert_cmid(cmid)?;
                        let ret_qp = if let Some((qp_handle, scq_handle, rcq_handle)) = handles {
                            Some(returned::QueuePair {
                                handle: interface::QueuePair(qp_handle),
                                send_cq: returned::CompletionQueue {
                                    handle: interface::CompletionQueue(scq_handle),
                                },
                                recv_cq: returned::CompletionQueue {
                                    handle: interface::CompletionQueue(rcq_handle),
                                },
                            })
                        } else {
                            None
                        };
                        let ret_cmid = returned::CmId {
                            handle: interface::CmId(cmid_handle),
                            qp: ret_qp,
                        };
                        Ok(ResponseKind::CreateEp(ret_cmid))
                    }
                    Err(e) => Err(Error::RdmaCm(e)),
                }
            }
            Request::Listen(cmid_handle, backlog) => {
                trace!("cmid_handle: {:?}, backlog: {}", cmid_handle, backlog);

                let listener = self.resource.cmid_table.get(cmid_handle)?;

                listener.listen(*backlog).map_err(Error::RdmaCm)?;

                Ok(ResponseKind::Listen)
            }
            Request::GetRequest(cmid_handle) => {
                trace!("cmid_handle: {:?}", cmid_handle);

                let listener = self.resource.cmid_table.get(cmid_handle)?;
                let new_cmid = listener.get_request().map_err(Error::RdmaCm)?;

                let (new_cmid_handle, handles) = self.resource.insert_cmid(new_cmid)?;
                let ret_qp = if let Some((qp_handle, scq_handle, rcq_handle)) = handles {
                    Some(returned::QueuePair {
                        handle: interface::QueuePair(qp_handle),
                        send_cq: returned::CompletionQueue {
                            handle: interface::CompletionQueue(scq_handle),
                        },
                        recv_cq: returned::CompletionQueue {
                            handle: interface::CompletionQueue(rcq_handle),
                        },
                    })
                } else {
                    None
                };
                let ret_cmid = returned::CmId {
                    handle: interface::CmId(new_cmid_handle),
                    qp: ret_qp,
                };
                Ok(ResponseKind::GetRequest(ret_cmid))
            }
            Request::Accept(cmid_handle, conn_param) => {
                trace!(
                    "cmid_handle: {:?}, conn_param: {:?}",
                    cmid_handle,
                    conn_param
                );
                warn!("TODO: conn_param is ignored for now");
                let listener = self.resource.cmid_table.get(cmid_handle)?;
                listener.accept().map_err(Error::RdmaCm)?;

                Ok(ResponseKind::Accept)
            }
            Request::Connect(cmid_handle, conn_param) => {
                trace!(
                    "cmid_handle: {:?}, conn_param: {:?}",
                    cmid_handle,
                    conn_param
                );
                warn!("TODO: conn_param is ignored for now");
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                cmid.connect().map_err(Error::RdmaCm)?;

                Ok(ResponseKind::Connect)
            }
            Request::RegMsgs(cmid_handle, addr_range) => {
                trace!(
                    "cmid_handle: {:?}, addr_range: {:#x?}",
                    cmid_handle,
                    addr_range
                );
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                // 1. create/open shared memory file
                use nix::fcntl::OFlag;
                use nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, MapFlags, ProtFlags};
                use nix::sys::stat::Mode;
                // just randomly pick an string and use that for now
                let shm_path = PathBuf::from(format!("koala-{}", Uuid::new_v4())); // no /dev/shm prefix is needed
                let fd = shm_open(
                    &shm_path,
                    OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
                    Mode::S_IRUSR | Mode::S_IWUSR,
                )
                .map_err(Error::ShmOpen)?;
                // 2. mmap the user's vaddr into koala's address space through
                // __linear__ mapping (this is necessary) (actually, maybe not)
                let uaddr = addr_range.start as usize;
                let ulen = (addr_range.end - addr_range.start) as usize;
                let page_size = 4096;
                let aligned_end = (uaddr + ulen + page_size - 1) / page_size * page_size;
                let aligned_begin = uaddr - uaddr % page_size;
                let aligned_len = aligned_end - aligned_begin;
                // ftruncate the file
                let memfile = unsafe { File::from_raw_fd(fd) };
                memfile.set_len(aligned_len as _).map_err(Error::Truncate)?;
                let kaddr = unsafe {
                    mmap(
                        ptr::null_mut(),
                        aligned_len,
                        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                        MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
                        fd,
                        0,
                    )
                    .map_err(Error::Mmap)?
                };
                // 3. send fd back
                ipc::send_fd(&self.sock, &self.client_path, fd).map_err(Error::SendFd)?;
                // 4. register kaddr with ibv_reg_mr
                let buf = unsafe { slice::from_raw_parts_mut(kaddr as *mut u8, aligned_len) };
                let mr = cmid.reg_msgs(&buf).map_err(Error::RdmaCm)?;
                // 5. allocate mr handle
                let new_mr_handle = mr.handle().into();
                self.resource.mr_table.insert(new_mr_handle, mr)?;
                // 6. allocate entry in MTT (mr -> SharedMemoryFile)
                let kbuf = &mut buf[uaddr - aligned_begin..uaddr - aligned_begin + ulen];
                let smf = SharedMemoryFile::new(addr_range.clone(), kbuf, memfile, &shm_path);
                self.mtt.allocate(new_mr_handle, smf);
                // 7. send mr handle back
                Ok(ResponseKind::RegMsgs(new_mr_handle))
            }
            _ => {
                unimplemented!()
            }
        }
    }
}
