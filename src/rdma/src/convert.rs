//! Convert ibv types and rdmacm types from and to koala types.
#![cfg(feature = "convert")]
use socket2::SockAddr;
use static_assertions::const_assert_eq;
use std::ffi::CStr;
use std::io;
use std::net::SocketAddr;
use std::os::raw::c_char;

use crate::ffi;
use crate::rdmacm;

mod sa {
    use super::*;
    use interface::addrinfo::AddrInfoFlags;
    const_assert_eq!(AddrInfoFlags::PASSIVE.bits(), ffi::RAI_PASSIVE);
    const_assert_eq!(AddrInfoFlags::NUMERICHOST.bits(), ffi::RAI_NUMERICHOST);
    const_assert_eq!(AddrInfoFlags::NOROUTE.bits(), ffi::RAI_NOROUTE);
    const_assert_eq!(AddrInfoFlags::FAMILY.bits(), ffi::RAI_FAMILY);
    // let mut flags: u32 = 0;
    // if h.flags.contains(AddrInfoFlags::PASSIVE) {
    //     flags |= ffi::RAI_PASSIVE;
    // }
    // if h.flags.contains(AddrInfoFlags::NUMERICHOST) {
    //     flags |= ffi::RAI_NUMERICHOST;
    // }
    // if h.flags.contains(AddrInfoFlags::NOROUTE) {
    //     flags |= ffi::RAI_NOROUTE;
    // }
    // if h.flags.contains(AddrInfoFlags::FAMILY) {
    //     flags |= ffi::RAI_FAMILY;
    // }
}

impl From<interface::addrinfo::AddrInfoHints> for rdmacm::AddrInfoHints {
    fn from(h: interface::addrinfo::AddrInfoHints) -> Self {
        use interface::addrinfo::AddrFamily;
        use interface::addrinfo::PortSpace;
        use interface::QpType;
        let flags = h.flags.bits();

        let family = h
            .family
            .map(|x| match x {
                AddrFamily::Inet => ffi::AF_INET,
                AddrFamily::Inet6 => ffi::AF_INET6,
                AddrFamily::Infiniband => ffi::AF_IB,
            })
            .unwrap_or(0);

        let qp_type = match h.qp_type {
            QpType::RC => ffi::ibv_qp_type::IBV_QPT_RC,
            QpType::UD => ffi::ibv_qp_type::IBV_QPT_UD,
        };

        let port_space = match h.port_space {
            PortSpace::IPOIB => ffi::rdma_port_space::RDMA_PS_IPOIB,
            PortSpace::TCP => ffi::rdma_port_space::RDMA_PS_TCP,
            PortSpace::UDP => ffi::rdma_port_space::RDMA_PS_UDP,
            PortSpace::IB => ffi::rdma_port_space::RDMA_PS_IB,
        };

        rdmacm::AddrInfoHints {
            flags: flags as _,
            family: family as _,
            qp_type: qp_type as _,
            port_space: port_space as _,
        }
    }
}

fn construct_socket_from_raw(
    addr: *mut ffi::sockaddr,
    socklen: ffi::socklen_t,
) -> io::Result<SocketAddr> {
    let ((), sockaddr) = unsafe {
        SockAddr::init(|storage, len| {
            *len = socklen;
            std::ptr::copy_nonoverlapping(addr as *const u8, storage as *mut u8, socklen as usize);
            Ok(())
        })
    }?;
    sockaddr.as_socket().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Found unknown address family: {}", sockaddr.family()),
        )
    })
}

fn from_c_str(cstr: *const c_char) -> Option<String> {
    unsafe {
        cstr.as_ref()
            .map(|s| CStr::from_ptr(s).to_str().unwrap().to_owned())
    }
}

impl From<rdmacm::AddrInfo> for interface::addrinfo::AddrInfo {
    fn from(ai: rdmacm::AddrInfo) -> Self {
        use interface::addrinfo::AddrFamily;
        use interface::addrinfo::AddrInfoFlags;
        use interface::addrinfo::PortSpace;
        use interface::QpType;

        let ai = unsafe { &*ai.0 };
        let flags = AddrInfoFlags::from_bits(ai.ai_flags as _).unwrap();
        let family = Some(match ai.ai_family as u32 {
            ffi::AF_INET => AddrFamily::Inet,
            ffi::AF_INET6 => AddrFamily::Inet6,
            ffi::AF_IB => AddrFamily::Infiniband,
            _ => panic!("ai_family: {}", ai.ai_family),
        });
        let qp_type = match ai.ai_qp_type as u32 {
            ffi::ibv_qp_type::IBV_QPT_RC => QpType::RC,
            ffi::ibv_qp_type::IBV_QPT_UD => QpType::UD,
            _ => panic!("ai_qp_type: {}", ai.ai_qp_type),
        };
        let port_space = match ai.ai_port_space as u32 {
            ffi::rdma_port_space::RDMA_PS_IPOIB => PortSpace::IPOIB,
            ffi::rdma_port_space::RDMA_PS_TCP => PortSpace::TCP,
            ffi::rdma_port_space::RDMA_PS_UDP => PortSpace::UDP,
            ffi::rdma_port_space::RDMA_PS_IB => PortSpace::IB,
            _ => panic!("ai_port_space: {}", ai.ai_port_space),
        };
        interface::addrinfo::AddrInfo {
            flags,
            family,
            qp_type,
            port_space,
            src_addr: construct_socket_from_raw(ai.ai_src_addr, ai.ai_src_len).unwrap(),
            src_canonname: from_c_str(ai.ai_src_canonname),
            dst_addr: construct_socket_from_raw(ai.ai_dst_addr, ai.ai_dst_len).unwrap(),
            dst_canonname: from_c_str(ai.ai_dst_canonname),
        }
    }
}
