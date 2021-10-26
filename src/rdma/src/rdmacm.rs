use std::ffi::CStr;
use std::fmt;
use std::io;
use std::mem::MaybeUninit;
use std::net::SocketAddr;
use std::os::raw::c_void;
use std::ptr;

use libc::{AI_ADDRCONFIG, AI_PASSIVE, AI_V4MAPPED};
use log::{debug, error, info, trace, warn};

use crate::ffi;
use crate::ibv;

#[derive(Debug)]
pub struct CmEvent(*mut ffi::rdma_cm_event);

/// All events which are allocated by rdma_get_cm_event must be released, there
/// should be a one-to-one correspondence  between  successful  gets  and  acks.
/// This call frees the event structure and any memory that it references.
impl Drop for CmEvent {
    fn drop(&mut self) {
        // ignore the error
        let _ = unsafe { ffi::rdma_ack_cm_event(self.0) };
    }
}

impl fmt::Display for CmEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = unsafe { CStr::from_ptr(ffi::rdma_event_str((*self.0).event)) };
        write!(f, "{}", msg.to_string_lossy())
    }
}

#[derive(Debug)]
pub struct EventChannel(*mut ffi::rdma_event_channel);

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
        let rc = unsafe { ffi::rdma_get_cm_event(self.0, &mut event as *mut _) };
        if rc != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(CmEvent(event))
        }
    }
}

impl Drop for EventChannel {
    fn drop(&mut self) {
        unsafe { ffi::rdma_destroy_event_channel(self.0) };
    }
}

#[derive(Debug)]
pub struct MemoryRegion(*mut ffi::ibv_mr);

#[derive(Debug)]
pub struct CmId(*mut ffi::rdma_cm_id);

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
    pub fn create_id(
        channel: Option<EventChannel>,
        context: usize,
        ps: ffi::rdma_port_space::Type,
    ) -> io::Result<CmId> {
        let channel = channel.map_or(ptr::null_mut(), |c| c.0);
        let mut cm_id: *mut ffi::rdma_cm_id = ptr::null_mut();
        let context = context as *mut c_void;

        let rc = unsafe { ffi::rdma_create_id(channel, &mut cm_id as *mut _, context, ps) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        assert!(!cm_id.is_null());
        Ok(CmId(cm_id))
    }

    pub fn bind_addr(&self, ai: dns_lookup::AddrInfo) -> io::Result<()> {
        let id = self.0;
        let addr = match ai.sockaddr {
            SocketAddr::V4(saddr) => &saddr as *const _ as *mut ffi::sockaddr,
            SocketAddr::V6(saddr) => &saddr as *const _ as *mut ffi::sockaddr,
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
        let rc = unsafe { ffi::rdma_get_request(id, &mut new_id as *mut _) };
        if rc != 0 || new_id.is_null() {
            return Err(io::Error::last_os_error());
        }

        assert!(!new_id.is_null());
        Ok(CmId(new_id))
    }

    pub fn accept(&self) -> io::Result<()> {
        let id = self.0;
        let conn_param = &mut ffi::rdma_conn_param {
            retry_count: 0,
            rnr_retry_count: 0,
            ..Default::default()
        };
        let rc = unsafe { ffi::rdma_accept(id, conn_param as *mut ffi::rdma_conn_param) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn resolve_addr(&self, ai: dns_lookup::AddrInfo) -> io::Result<()> {
        let id = self.0;
        let src_addr = ptr::null_mut();
        let dst_addr = match ai.sockaddr {
            SocketAddr::V4(saddr) => &saddr as *const _ as *mut ffi::sockaddr,
            SocketAddr::V6(saddr) => &saddr as *const _ as *mut ffi::sockaddr,
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

    pub fn create_qp(&self) -> io::Result<()> {
        let id = self.0;
        let pd = ptr::null_mut();
        let qp_init_attr = &mut ffi::ibv_qp_init_attr {
            qp_context: ptr::null_mut(),
            send_cq: ptr::null_mut(),
            recv_cq: ptr::null_mut(),
            cap: ffi::ibv_qp_cap {
                max_send_wr: 128,
                max_recv_wr: 128,
                max_send_sge: 5,
                max_recv_sge: 5,
                max_inline_data: 128,
            },
            qp_type: ffi::ibv_qp_type::IBV_QPT_RC,
            sq_sig_all: 0,
            ..Default::default()
        };
        let rc = unsafe { ffi::rdma_create_qp(id, pd, qp_init_attr as *mut ffi::ibv_qp_init_attr) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn connect(&self) -> io::Result<()> {
        let id = self.0;
        let conn_param = &mut ffi::rdma_conn_param {
            retry_count: 0,
            rnr_retry_count: 0,
            ..Default::default()
        };
        let rc = unsafe { ffi::rdma_connect(id, conn_param as *mut ffi::rdma_conn_param) };
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

    #[inline]
    pub unsafe fn post_send(&self, buf: &[u8], mr: &MemoryRegion) -> io::Result<()> {
        let id = self.0;
        let context = ptr::null_mut();
        let addr = buf.as_ptr();
        let length = buf.len();

        let mr = mr.0;
        assert!(!mr.is_null());
        assert!(
            (&*mr).addr as *const _ <= addr
                && addr.add(length) <= (&*mr).addr.add((&*mr).length as usize) as *const _
        );
        let flags = ffi::ibv_send_flags::IBV_SEND_SIGNALED;
        let rc =
            ffi::rdma_post_send_real(id, context, addr as *mut _, length as u64, mr, flags.0 as _);
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[inline]
    pub unsafe fn post_recv(&self, buf: &[u8], mr: &MemoryRegion) -> io::Result<()> {
        let id = self.0;
        let context = ptr::null_mut();
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
