use std::ffi::{CStr, CString};
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::mem;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::net::SocketAddr;
use std::os::raw::{c_char, c_void};
use std::os::unix::io::{AsRawFd, RawFd};
use std::ptr;
use std::slice;

use log::warn;
use socket2::SockAddr;

use crate::ffi;
use crate::ibv;

#[derive(Debug, Clone, Copy)]
pub struct AddrInfoHints {
    pub(crate) flags: i32,
    pub(crate) family: i32,
    pub(crate) qp_type: i32,
    pub(crate) port_space: i32,
}

impl AddrInfoHints {
    pub fn new(
        flags: Option<i32>,
        family: Option<i32>,
        qp_type: Option<i32>,
        port_space: Option<i32>,
    ) -> Self {
        AddrInfoHints {
            flags: flags.unwrap_or(0),
            family: family.unwrap_or(0),
            qp_type: qp_type.unwrap_or(0),
            port_space: port_space.unwrap_or(0),
        }
    }

    pub fn to_addrinfo(&self) -> ffi::rdma_addrinfo {
        let mut ai: ffi::rdma_addrinfo = unsafe { mem::zeroed() };
        ai.ai_flags = self.flags;
        ai.ai_family = self.family;
        ai.ai_qp_type = self.qp_type;
        ai.ai_port_space = self.port_space;
        ai
    }
}

#[derive(Debug)]
pub struct AddrInfo {
    pub ai_flags: i32,
    pub ai_family: i32,
    pub ai_qp_type: i32,
    pub ai_port_space: i32,
    pub ai_src_addr: Option<SockAddr>,
    pub ai_dst_addr: Option<SockAddr>,
    pub ai_src_canonname: Option<CString>,
    pub ai_dst_canonname: Option<CString>,
    pub ai_route: Vec<u8>,
    pub ai_connect: Vec<u8>,
}

/// Safety: Caller must ensure that the address family and length match the type of storage
/// address. For example if storage.ss_family is set to AF_INET the storage must be initialised as
/// sockaddr_in, setting the content and length appropriately.
unsafe fn sockaddr_from_raw(
    addr: *mut ffi::sockaddr,
    socklen: ffi::socklen_t,
) -> io::Result<SockAddr> {
    let ((), sockaddr) = SockAddr::init(|storage, len| {
        *len = socklen;
        std::ptr::copy_nonoverlapping(addr as *const u8, storage as *mut u8, socklen as usize);
        Ok(())
    })?;

    if sockaddr.as_socket().is_none() {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Found unknown address family: {}", sockaddr.family()),
        ))
    } else {
        Ok(sockaddr)
    }
}

/// Safety: null pointer is checked. Data is copied to a new place, so lifetime won't be a issue.
/// Warning: there's no way to know if the input pointer is valid throughout this invocation.
unsafe fn from_c_str(cstr: *const c_char) -> Option<CString> {
    cstr.as_ref().map(|s| CStr::from_ptr(s).to_owned())
}

impl AddrInfo {
    pub fn getaddrinfo(
        node: Option<&str>,
        service: Option<&str>,
        hints: Option<&AddrInfoHints>,
    ) -> io::Result<AddrInfo> {
        let node = node.map(|s| CString::new(s).unwrap());
        let c_node = node.as_ref().map_or(ptr::null(), |s| s.as_ptr());
        let service = service.map(|s| CString::new(s).unwrap());
        let c_service = service.as_ref().map_or(ptr::null(), |s| s.as_ptr());
        let hints = hints.map(|h| h.to_addrinfo());
        let c_hints = hints.as_ref().map_or(ptr::null(), |h| h as *const _);
        let mut res = ptr::null_mut();
        let rc = unsafe { ffi::rdma_getaddrinfo(c_node, c_service, c_hints, &mut res) };
        match rc {
            0 => {
                let ret = unsafe { Self::from_ptr(res) };
                unsafe {
                    ffi::rdma_freeaddrinfo(res);
                }
                ret
            }
            -1 => Err(io::Error::last_os_error()),
            _ => Err(io::Error::from_raw_os_error(rc)),
        }
    }

    pub unsafe fn from_ptr(a: *const ffi::rdma_addrinfo) -> io::Result<AddrInfo> {
        if a.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Supplied pointer is null.",
            ));
        }
        let a = *a;
        // The underlying API should not returns an addrinfo with non-null ai_next
        assert!(a.ai_next.is_null());
        let ai_src_addr = sockaddr_from_raw(a.ai_src_addr, a.ai_src_len).ok();
        let ai_dst_addr = sockaddr_from_raw(a.ai_dst_addr, a.ai_dst_len).ok();
        let ai_src_canonname = from_c_str(a.ai_src_canonname);
        let ai_dst_canonname = from_c_str(a.ai_dst_canonname);
        let ai_route =
            slice::from_raw_parts(a.ai_route as *const u8, a.ai_route_len as usize).to_vec();
        let ai_connect =
            slice::from_raw_parts(a.ai_connect as *const u8, a.ai_connect_len as usize).to_vec();
        Ok(AddrInfo {
            ai_flags: a.ai_flags,
            ai_family: a.ai_family,
            ai_qp_type: a.ai_qp_type,
            ai_port_space: a.ai_port_space,
            ai_src_addr,
            ai_dst_addr,
            ai_src_canonname,
            ai_dst_canonname,
            ai_route,
            ai_connect,
        })
    }

    pub fn as_addrinfo<'a>(&'a self) -> AddrInfoTransparent<'a> {
        let mut ai: ffi::rdma_addrinfo = unsafe { mem::zeroed() };
        ai.ai_flags = self.ai_flags;
        ai.ai_family = self.ai_family;
        ai.ai_qp_type = self.ai_qp_type;
        ai.ai_port_space = self.ai_port_space;
        ai.ai_src_len = self.ai_src_addr.as_ref().map_or(0, |s| s.len());
        ai.ai_src_addr = self
            .ai_src_addr
            .as_ref()
            .map_or(ptr::null_mut(), |s| s.as_ptr() as _);
        ai.ai_dst_len = self.ai_dst_addr.as_ref().map_or(0, |s| s.len());
        ai.ai_dst_addr = self
            .ai_dst_addr
            .as_ref()
            .map_or(ptr::null_mut(), |s| s.as_ptr() as _);
        ai.ai_src_canonname = self
            .ai_src_canonname
            .as_ref()
            .map_or(ptr::null_mut(), |s| s.as_ptr() as _);
        ai.ai_dst_canonname = self
            .ai_dst_canonname
            .as_ref()
            .map_or(ptr::null_mut(), |s| s.as_ptr() as _);
        ai.ai_route_len = self.ai_route.len() as _;
        ai.ai_route = if self.ai_route.is_empty() {
            ptr::null_mut()
        } else {
            self.ai_route.as_ptr() as _
        };
        ai.ai_connect_len = self.ai_connect.len() as _;
        ai.ai_route = if self.ai_connect.is_empty() {
            ptr::null_mut()
        } else {
            self.ai_connect.as_ptr() as _
        };
        ai.ai_next = ptr::null_mut();
        AddrInfoTransparent {
            inner: ai,
            _marker: PhantomData,
        }
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub struct AddrInfoTransparent<'a> {
    inner: ffi::rdma_addrinfo,
    _marker: PhantomData<&'a AddrInfo>,
}

impl<'a> AsRef<ffi::rdma_addrinfo> for AddrInfoTransparent<'a> {
    fn as_ref(&self) -> &ffi::rdma_addrinfo {
        &self.inner
    }
}

impl<'a> AsMut<ffi::rdma_addrinfo> for AddrInfoTransparent<'a> {
    fn as_mut(&mut self) -> &mut ffi::rdma_addrinfo {
        &mut self.inner
    }
}

pub struct ContextList(&'static mut [ManuallyDrop<ibv::Context>]);

unsafe impl Sync for ContextList {}
unsafe impl Send for ContextList {}

impl Drop for ContextList {
    fn drop(&mut self) {
        unsafe { ffi::rdma_free_devices(self.0.as_mut_ptr() as _) };
    }
}

use std::ops::Deref;
impl Deref for ContextList {
    type Target = [ManuallyDrop<ibv::Context>];
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl ContextList {
    pub fn drain(self) -> ContextListDrain {
        self.into_iter()
    }
}

pub struct ContextListDrain {
    list: ContextList,
    i: usize,
}

impl IntoIterator for ContextList {
    type Item = <ContextListDrain as Iterator>::Item;
    type IntoIter = ContextListDrain;
    fn into_iter(self) -> Self::IntoIter {
        ContextListDrain { list: self, i: 0 }
    }
}

impl Iterator for ContextListDrain {
    type Item = ManuallyDrop<ibv::Context>;
    fn next(&mut self) -> Option<Self::Item> {
        let e = self.list.get(self.i);
        if e.is_some() {
            self.i += 1;
        }
        e.map(|e| ManuallyDrop::new(ibv::Context { ctx: e.ctx }))
    }
}

pub fn get_devices() -> io::Result<ContextList> {
    // use rdma_get_devices(), the ibv_context* will remain valid after
    // rdma_free_deivces is called.
    let mut n = 0i32;
    let contexts = unsafe { ffi::rdma_get_devices(&mut n) };
    if contexts.is_null() {
        return Err(io::Error::last_os_error());
    }
    assert_eq!(
        mem::size_of::<ibv::Context>(),
        mem::size_of::<*mut ffi::ibv_context>()
    );
    let contexts = unsafe { slice::from_raw_parts_mut(contexts as _, n as usize) };
    Ok(ContextList(contexts))
}

#[derive(Debug, Clone, Copy)]
pub struct PortSpace(pub ffi::rdma_port_space::Type);

#[repr(transparent)]
#[derive(Debug)]
pub struct CmEvent(*mut ffi::rdma_cm_event);

unsafe impl Send for CmEvent {}
unsafe impl Sync for CmEvent {}

/// All events which are allocated by rdma_get_cm_event must be released, there
/// should be a one-to-one correspondence  between  successful  gets  and  acks.
/// This call frees the event structure and any memory that it references.
impl Drop for CmEvent {
    fn drop(&mut self) {
        // ignore the error
        let rc = unsafe { ffi::rdma_ack_cm_event(self.0) };
        if rc != 0 {
            warn!(
                "An error occurred on ack_cm_event: {:?}",
                io::Error::last_os_error()
            );
        }
    }
}

impl fmt::Display for CmEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = unsafe { CStr::from_ptr(ffi::rdma_event_str((*self.0).event)) };
        write!(f, "{}", msg.to_string_lossy())
    }
}

impl CmEvent {
    #[inline]
    pub fn status(&self) -> i32 {
        assert!(!self.0.is_null());
        unsafe { &*self.0 }.status
    }

    #[inline]
    pub fn event(&self) -> ffi::rdma_cm_event_type::Type {
        assert!(!self.0.is_null());
        unsafe { &*self.0 }.event
    }

    #[inline]
    pub fn id(&self) -> CmId {
        assert!(!self.0.is_null());
        let id = unsafe { &*self.0 }.id;
        assert!(!id.is_null());
        CmId(id)
    }

    #[inline]
    pub fn listen_id(&self) -> Option<CmId> {
        assert!(!self.0.is_null());
        let listen_id = unsafe { &*self.0 }.listen_id;
        if listen_id.is_null() {
            None
        } else {
            Some(CmId(listen_id))
        }
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub struct EventChannel(*mut ffi::rdma_event_channel);

unsafe impl Send for EventChannel {}
unsafe impl Sync for EventChannel {}

impl AsRawFd for EventChannel {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        assert!(!self.0.is_null());
        unsafe { &*self.0 }.fd
    }
}

impl EventChannel {
    pub fn create_event_channel() -> io::Result<Self> {
        let channel = unsafe { ffi::rdma_create_event_channel() };
        if channel.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(EventChannel(channel))
        }
    }

    pub fn get_cm_event(&self) -> io::Result<CmEvent> {
        let mut event = ptr::null_mut();
        let rc = unsafe { ffi::rdma_get_cm_event(self.0, &mut event) };
        if rc != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(CmEvent(event))
        }
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        let mut flags = unsafe { libc::fcntl(self.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if nonblocking {
            flags |= libc::O_NONBLOCK;
        } else {
            flags &= !libc::O_NONBLOCK;
        }
        let rc = unsafe { libc::fcntl(self.as_raw_fd(), libc::F_SETFL, flags) };
        if rc == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn is_nonblocking(&self) -> io::Result<bool> {
        let flags = unsafe { libc::fcntl(self.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(flags & libc::O_NONBLOCK > 0)
    }

    #[inline]
    pub fn handle(&self) -> u32 {
        self.as_raw_fd() as _
    }
}

impl Drop for EventChannel {
    fn drop(&mut self) {
        unsafe { ffi::rdma_destroy_event_channel(self.0) };
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub struct MemoryRegion(pub(crate) *mut ffi::ibv_mr);

unsafe impl Send for MemoryRegion {}
unsafe impl Sync for MemoryRegion {}

impl MemoryRegion {
    // TODO(cjr): Replace this with proc macro
    pub fn handle(&self) -> u32 {
        assert!(!self.0.is_null());
        unsafe { &*self.0 }.handle
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub struct CmId(*mut ffi::rdma_cm_id);

unsafe impl Send for CmId {}
unsafe impl Sync for CmId {}

impl Drop for CmId {
    fn drop(&mut self) {
        let rc = unsafe { ffi::rdma_destroy_id(self.0) };
        if rc != 0 {
            warn!(
                "error occured when destroying cm_id: {:?}",
                io::Error::last_os_error()
            );
        }
    }
}

impl CmId {
    // TODO(cjr): fix this bug.
    #[inline]
    pub fn qp<'res>(&self) -> Option<ibv::QueuePair<'res>> {
        assert!(!self.0.is_null());
        let qp = unsafe { &*self.0 }.qp;
        if qp.is_null() {
            None
        } else {
            Some(ibv::QueuePair {
                _phantom: PhantomData,
                qp,
            })
        }
    }

    #[inline]
    pub fn event_channel(&self) -> EventChannel {
        assert!(!self.0.is_null());
        let channel = unsafe { &*self.0 }.channel;
        assert!(!channel.is_null());
        EventChannel(channel)
    }

    #[inline]
    pub fn sgid(&self) -> ibv::Gid {
        assert!(!self.0.is_null());
        let route = unsafe { &*self.0 }.route;
        let sgid = unsafe { route.addr.addr.ibaddr.sgid }.into();
        sgid
    }

    pub fn create_ep<'ctx>(
        ai: &AddrInfo,
        pd: Option<&ibv::ProtectionDomain<'ctx>>,
        qp_init_attr: Option<&ffi::ibv_qp_init_attr>,
    ) -> io::Result<CmId> {
        let mut cm_id = ptr::null_mut();
        let mut a = ai.as_addrinfo();
        let rc = unsafe {
            ffi::rdma_create_ep(
                &mut cm_id,
                a.as_mut(),
                pd.map_or(ptr::null_mut(), |pd| pd.pd),
                qp_init_attr.map_or(ptr::null_mut(), |a| a as *const _ as *mut _),
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        assert!(!cm_id.is_null());
        Ok(CmId(cm_id))
    }

    pub fn create_id(
        channel: Option<&EventChannel>,
        context: usize,
        ps: ffi::rdma_port_space::Type,
    ) -> io::Result<CmId> {
        let channel = channel.map_or(ptr::null_mut(), |c| c.0);
        let mut cm_id: *mut ffi::rdma_cm_id = ptr::null_mut();
        let context = context as *mut c_void;

        let rc = unsafe { ffi::rdma_create_id(channel, &mut cm_id, context, ps) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        assert!(!cm_id.is_null());
        Ok(CmId(cm_id))
    }

    pub fn bind_addr(&self, sockaddr: &SocketAddr) -> io::Result<()> {
        let id = self.0;
        let addr = match sockaddr {
            SocketAddr::V4(saddr) => saddr as *const _ as *mut ffi::sockaddr,
            SocketAddr::V6(saddr) => saddr as *const _ as *mut ffi::sockaddr,
        };
        let rc = unsafe { ffi::rdma_bind_addr(id, addr) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub fn listen(&self, backlog: i32) -> io::Result<()> {
        let id = self.0;
        let rc = unsafe { ffi::rdma_listen(id, backlog) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub fn get_request(&self) -> io::Result<CmId> {
        let id = self.0;
        let mut new_id: *mut ffi::rdma_cm_id = ptr::null_mut();
        let rc = unsafe { ffi::rdma_get_request(id, &mut new_id) };
        if rc != 0 || new_id.is_null() {
            return Err(io::Error::last_os_error());
        }

        assert!(!new_id.is_null());
        Ok(CmId(new_id))
    }

    pub fn accept(&self, conn_param: Option<&ffi::rdma_conn_param>) -> io::Result<()> {
        let id = self.0;
        let rc = unsafe {
            ffi::rdma_accept(
                id,
                conn_param.map_or(ptr::null_mut(), |param| param as *const _ as *mut _),
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn resolve_addr(&self, sockaddr: &SocketAddr) -> io::Result<()> {
        let id = self.0;
        let src_addr = ptr::null_mut();
        let dst_addr = match sockaddr {
            SocketAddr::V4(saddr) => saddr as *const _ as *mut ffi::sockaddr,
            SocketAddr::V6(saddr) => saddr as *const _ as *mut ffi::sockaddr,
        };
        let timeout_ms = 1500;

        let rc = unsafe { ffi::rdma_resolve_addr(id, src_addr, dst_addr, timeout_ms) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub fn resolve_route(&self, timeout_ms: i32) -> io::Result<()> {
        let id = self.0;
        let rc = unsafe { ffi::rdma_resolve_route(id, timeout_ms) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn create_qp<'ctx>(
        &self,
        pd: Option<&ibv::ProtectionDomain<'ctx>>,
        qp_init_attr: Option<&ffi::ibv_qp_init_attr>,
    ) -> io::Result<()> {
        let id = self.0;
        assert!(!id.is_null());
        let rc = unsafe {
            ffi::rdma_create_qp(
                id,
                pd.map_or(ptr::null_mut(), |pd| pd.pd),
                qp_init_attr.map_or(ptr::null_mut(), |a| a as *const _ as *mut _),
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn connect(&self, conn_param: Option<&ffi::rdma_conn_param>) -> io::Result<()> {
        let id = self.0;
        let rc = unsafe {
            ffi::rdma_connect(
                id,
                conn_param.map_or(ptr::null_mut(), |param| param as *const _ as *mut _),
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[inline]
    pub fn reg_msgs(&self, buf: &[u8]) -> io::Result<MemoryRegion> {
        let id = self.0;
        let addr = buf.as_ptr();
        let length = buf.len();
        let mr = unsafe { ffi::rdma_reg_msgs_real(id, addr as *mut _, length as u64) };
        if mr.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(MemoryRegion(mr))
    }

    pub fn disconnect(&self) -> io::Result<()> {
        let id = self.0;
        let rc = unsafe { ffi::rdma_disconnect(id) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[inline]
    pub unsafe fn post_send(
        &self,
        wr_id: u64,
        buf: &[u8],
        mr: &MemoryRegion,
        flags: ffi::ibv_send_flags,
    ) -> io::Result<()> {
        let id = self.0;
        let context = wr_id as _;
        let addr = buf.as_ptr();
        let length = buf.len();

        let mr = mr.0;
        assert!(!mr.is_null());
        assert!(
            (&*mr).addr as *const _ <= addr
                && addr.add(length) <= (&*mr).addr.add((&*mr).length as usize) as *const _
        );
        let rc =
            ffi::rdma_post_send_real(id, context, addr as *mut _, length as u64, mr, flags.0 as _);
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[inline]
    pub unsafe fn post_recv(
        &self,
        wr_id: u64,
        buf: &mut [u8],
        mr: &MemoryRegion,
    ) -> io::Result<()> {
        let id = self.0;
        let context = wr_id as _;
        let addr = buf.as_ptr();
        let length = buf.len();

        let mr = mr.0;
        assert!(!mr.is_null());
        assert!(
            (&*mr).addr as *const _ <= addr
                && addr.add(length) <= (&*mr).addr.add((&*mr).length as usize) as *const _
        );
        let rc = ffi::rdma_post_recv_real(id, context, addr as *mut _, length as u64, mr);
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[inline]
    pub unsafe fn post_write(
        &self,
        wr_id: u64,
        buf: &[u8],
        mr: &MemoryRegion,
        flags: ffi::ibv_send_flags,
        remote_addr: u64,
        rkey: u32,
    ) -> io::Result<()> {
        let id = self.0;
        let context = wr_id as _;
        let addr = buf.as_ptr();
        let length = buf.len();

        let mr = mr.0;
        assert!(!mr.is_null());
        assert!(
            (&*mr).addr as *const _ <= addr
                && addr.add(length) <= (&*mr).addr.add((&*mr).length as usize) as *const _
        );
        let rc = ffi::rdma_post_write_real(
            id,
            context,
            addr as *mut _,
            length as u64,
            mr,
            flags.0 as _,
            remote_addr,
            rkey,
        );
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[inline]
    pub unsafe fn post_read(
        &self,
        wr_id: u64,
        buf: &mut [u8],
        mr: &MemoryRegion,
        flags: ffi::ibv_send_flags,
        remote_addr: u64,
        rkey: u32,
    ) -> io::Result<()> {
        let id = self.0;
        let context = wr_id as _;
        let addr = buf.as_ptr();
        let length = buf.len();

        let mr = mr.0;
        assert!(!mr.is_null());
        assert!(
            (&*mr).addr as *const _ <= addr
                && addr.add(length) <= (&*mr).addr.add((&*mr).length as usize) as *const _
        );
        let rc = ffi::rdma_post_read_real(
            id,
            context,
            addr as *mut _,
            length as u64,
            mr,
            flags.0 as _,
            remote_addr,
            rkey,
        );
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[inline]
    pub fn get_send_comp(&self) -> io::Result<ffi::ibv_wc> {
        let id = self.0;
        let mut wc = MaybeUninit::uninit();
        let rc = unsafe { ffi::rdma_get_send_comp_real(id, wc.as_mut_ptr()) };
        if rc != 1 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { wc.assume_init() })
    }

    #[inline]
    pub fn get_recv_comp(&self) -> io::Result<ffi::ibv_wc> {
        let id = self.0;
        let mut wc = MaybeUninit::uninit();
        let rc = unsafe { ffi::rdma_get_recv_comp_real(id, wc.as_mut_ptr()) };
        if rc != 1 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { wc.assume_init() })
    }
}
