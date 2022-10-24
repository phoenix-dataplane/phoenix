use std::mem;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use crate::ffi;

pub(crate) trait IntoInner<T> {
    fn into_inner(self) -> T;
}

pub(crate) trait FromInner<T> {
    fn from_inner(_: T) -> Self;
}

impl FromInner<ffi::sockaddr_in> for SocketAddrV4 {
    #[inline]
    fn from_inner(addr: ffi::sockaddr_in) -> SocketAddrV4 {
        SocketAddrV4::new(
            Ipv4Addr::from_inner(addr.sin_addr),
            u16::from_be(addr.sin_port),
        )
    }
}

impl FromInner<ffi::sockaddr_in6> for SocketAddrV6 {
    #[inline]
    fn from_inner(addr: ffi::sockaddr_in6) -> SocketAddrV6 {
        SocketAddrV6::new(
            Ipv6Addr::from_inner(addr.sin6_addr),
            u16::from_be(addr.sin6_port),
            addr.sin6_flowinfo,
            addr.sin6_scope_id,
        )
    }
}

impl IntoInner<ffi::sockaddr_in> for SocketAddrV4 {
    #[inline]
    fn into_inner(self) -> ffi::sockaddr_in {
        ffi::sockaddr_in {
            sin_family: ffi::AF_INET as ffi::sa_family_t,
            sin_port: self.port().to_be(),
            sin_addr: self.ip().into_inner(),
            ..unsafe { mem::zeroed() }
        }
    }
}

impl IntoInner<ffi::sockaddr_in6> for SocketAddrV6 {
    #[inline]
    fn into_inner(self) -> ffi::sockaddr_in6 {
        ffi::sockaddr_in6 {
            sin6_family: ffi::AF_INET6 as ffi::sa_family_t,
            sin6_port: self.port().to_be(),
            sin6_addr: self.ip().into_inner(),
            sin6_flowinfo: self.flowinfo(),
            sin6_scope_id: self.scope_id(),
        }
    }
}

impl IntoInner<ffi::in_addr> for Ipv4Addr {
    #[inline]
    fn into_inner(self) -> ffi::in_addr {
        // `s_addr` is stored as BE on all machines and the array is in BE order.
        // So the native endian conversion method is used so that it's never swapped.
        ffi::in_addr {
            s_addr: u32::from_ne_bytes(self.octets()),
        }
    }
}
impl FromInner<ffi::in_addr> for Ipv4Addr {
    #[inline]
    fn from_inner(addr: ffi::in_addr) -> Ipv4Addr {
        Ipv4Addr::from(addr.s_addr.to_ne_bytes())
    }
}

impl IntoInner<ffi::in6_addr> for Ipv6Addr {
    #[inline]
    fn into_inner(self) -> ffi::in6_addr {
        ffi::in6_addr {
            __in6_u: ffi::in6_addr__bindgen_ty_1 {
                __u6_addr8: self.octets(),
            },
        }
    }
}

impl FromInner<ffi::in6_addr> for Ipv6Addr {
    #[inline]
    fn from_inner(addr: ffi::in6_addr) -> Ipv6Addr {
        // SAFETY: all union variants points to the same memory, with different layouts. We use the
        // byte-based one.
        Ipv6Addr::from(unsafe { addr.__in6_u.__u6_addr8 })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Converting SocketAddr to libc representation
////////////////////////////////////////////////////////////////////////////////

/// A type with the same memory layout as `ffi::sockaddr`. Used in converting Rust level
/// SocketAddr* types into their system representation. The benefit of this specific
/// type over using `ffi::sockaddr_storage` is that this type is exactly as large as it
/// needs to be and not a lot larger. And it can be initialized more cleanly from Rust.
#[repr(C)]
pub(crate) union SocketAddrCRepr {
    v4: ffi::sockaddr_in,
    v6: ffi::sockaddr_in6,
}

impl SocketAddrCRepr {
    #[allow(unused)]
    #[inline]
    pub(crate) fn as_ptr(&self) -> *const ffi::sockaddr {
        self as *const _ as *const ffi::sockaddr
    }

    #[inline]
    pub(crate) fn as_mut_ptr(&mut self) -> *mut ffi::sockaddr {
        self as *mut _ as *mut ffi::sockaddr
    }
}

impl<'a> IntoInner<(SocketAddrCRepr, ffi::socklen_t)> for &'a SocketAddr {
    #[inline]
    fn into_inner(self) -> (SocketAddrCRepr, ffi::socklen_t) {
        match *self {
            SocketAddr::V4(ref a) => {
                let sockaddr = SocketAddrCRepr { v4: a.into_inner() };
                (
                    sockaddr,
                    mem::size_of::<ffi::sockaddr_in>() as ffi::socklen_t,
                )
            }
            SocketAddr::V6(ref a) => {
                let sockaddr = SocketAddrCRepr { v6: a.into_inner() };
                (
                    sockaddr,
                    mem::size_of::<ffi::sockaddr_in6>() as ffi::socklen_t,
                )
            }
        }
    }
}
