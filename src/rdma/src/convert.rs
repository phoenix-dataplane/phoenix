//! Convert ibv types and rdmacm types from and to koala types.
#![cfg(feature = "convert")]
use socket2::SockAddr;
use static_assertions::const_assert_eq;
use std::ffi::CString;

use crate::ffi;
use crate::ibv;
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
    use interface::WcFlags;
    const_assert_eq!(WcFlags::GRH.bits(), ffi::ibv_wc_flags::IBV_WC_GRH.0);
    const_assert_eq!(
        WcFlags::WITH_IMM.bits(),
        ffi::ibv_wc_flags::IBV_WC_WITH_IMM.0
    );
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

impl From<rdmacm::AddrInfo> for interface::addrinfo::AddrInfo {
    fn from(other: rdmacm::AddrInfo) -> Self {
        use interface::addrinfo::AddrFamily;
        use interface::addrinfo::AddrInfoFlags;
        use interface::addrinfo::PortSpace;
        use interface::QpType;

        let ai = other;
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
        let mut buffer = Vec::new();
        bincode::serialize_into(&mut buffer, &(ai.ai_route, ai.ai_connect))
            .expect("serialize_into");
        interface::addrinfo::AddrInfo {
            flags,
            family,
            qp_type,
            port_space,
            src_addr: ai.ai_src_addr.map(|s| s.as_socket().unwrap()), // don't fail silently
            dst_addr: ai.ai_dst_addr.map(|s| s.as_socket().unwrap()),
            src_canonname: ai.ai_src_canonname.map(|s| s.into_string().unwrap()),
            dst_canonname: ai.ai_dst_canonname.map(|s| s.into_string().unwrap()),
            payload: buffer,
        }
    }
}

impl From<interface::addrinfo::AddrInfo> for rdmacm::AddrInfo {
    fn from(other: interface::addrinfo::AddrInfo) -> Self {
        let hints = interface::addrinfo::AddrInfoHints::new(
            other.flags,
            other.family,
            other.qp_type,
            other.port_space,
        );
        let hints = rdmacm::AddrInfoHints::from(hints);
        let (route, connect_data) = bincode::deserialize(&other.payload).expect("deserialize_from");
        rdmacm::AddrInfo {
            ai_flags: hints.flags,
            ai_family: hints.family,
            ai_qp_type: hints.qp_type,
            ai_port_space: hints.port_space,
            ai_src_addr: other.src_addr.map(SockAddr::from),
            ai_dst_addr: other.dst_addr.map(SockAddr::from),
            ai_src_canonname: other.src_canonname.and_then(|s| CString::new(s).ok()),
            ai_dst_canonname: other.dst_canonname.and_then(|s| CString::new(s).ok()),
            ai_route: route,
            ai_connect: connect_data,
        }
    }
}

impl From<interface::QpCapability> for ibv::QpCapability {
    fn from(other: interface::QpCapability) -> Self {
        let inner = ffi::ibv_qp_cap {
            max_send_wr: other.max_send_wr,
            max_recv_wr: other.max_recv_wr,
            max_send_sge: other.max_send_sge,
            max_recv_sge: other.max_recv_sge,
            max_inline_data: other.max_inline_data,
        };
        ibv::QpCapability(inner)
    }
}

impl From<interface::QpType> for ibv::QpType {
    fn from(other: interface::QpType) -> Self {
        use interface::QpType::*;
        let inner = match other {
            RC => ffi::ibv_qp_type::IBV_QPT_RC,
            UD => ffi::ibv_qp_type::IBV_QPT_UD,
        };
        ibv::QpType(inner)
    }
}

impl From<ffi::ibv_wc> for interface::WorkCompletion {
    fn from(other: ffi::ibv_wc) -> Self {
        use ffi::ibv_wc_opcode;
        use ffi::ibv_wc_status;
        use interface::WcOpcode;
        use interface::WcStatus;
        let status = match other.status {
            ibv_wc_status::IBV_WC_SUCCESS => WcStatus::Success,
            e @ _ => WcStatus::Error(e),
        };

        // If status is ERR, the opcode and some other fields might be invalid.
        let opcode = if other.status == ibv_wc_status::IBV_WC_SUCCESS {
            // This conversion only valid when status is success.
            match other.opcode {
                ibv_wc_opcode::IBV_WC_SEND => WcOpcode::Send,
                ibv_wc_opcode::IBV_WC_RDMA_WRITE => WcOpcode::RdmaWrite,
                ibv_wc_opcode::IBV_WC_RDMA_READ => WcOpcode::RdmaRead,
                ibv_wc_opcode::IBV_WC_RECV => WcOpcode::Recv,
                ibv_wc_opcode::IBV_WC_RECV_RDMA_WITH_IMM => WcOpcode::RecvRdmaWithImm,
                code @ _ => panic!("unimplemented opcode: {:?}, wc: {:?}", code, other),
            }
        } else {
            WcOpcode::Invalid
        };

        let wc_flags = interface::WcFlags::from_bits(other.wc_flags.0).unwrap();

        interface::WorkCompletion {
            wr_id: other.wr_id,
            status,
            opcode,
            vendor_err: other.vendor_err,
            byte_len: other.byte_len,
            imm_data: other.imm_data,
            wc_flags,
        }
    }
}
