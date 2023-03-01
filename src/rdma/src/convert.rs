//! Convert ibv types and rdmacm types from and to phoenix types.
#![cfg(feature = "phoenix")]
use std::num::NonZeroU32;

use socket2::SockAddr;
use static_assertions::const_assert_eq;
use std::ffi::CString;

use crate::ffi;
use crate::ibv;
use crate::rdmacm;

mod sa {
    use super::*;
    use phoenix_api::addrinfo::AddrInfoFlags;
    const_assert_eq!(AddrInfoFlags::PASSIVE.bits(), ffi::RAI_PASSIVE);
    const_assert_eq!(AddrInfoFlags::NUMERICHOST.bits(), ffi::RAI_NUMERICHOST);
    const_assert_eq!(AddrInfoFlags::NOROUTE.bits(), ffi::RAI_NOROUTE);
    const_assert_eq!(AddrInfoFlags::FAMILY.bits(), ffi::RAI_FAMILY);
    use phoenix_api::net::WcFlags;
    const_assert_eq!(WcFlags::GRH.bits(), ffi::ibv_wc_flags::IBV_WC_GRH.0);
    const_assert_eq!(
        WcFlags::WITH_IMM.bits(),
        ffi::ibv_wc_flags::IBV_WC_WITH_IMM.0
    );

    use phoenix_api::net::SendFlags;
    const_assert_eq!(
        SendFlags::FENCE.bits(),
        ffi::ibv_send_flags::IBV_SEND_FENCE.0
    );
    const_assert_eq!(
        SendFlags::SIGNALED.bits(),
        ffi::ibv_send_flags::IBV_SEND_SIGNALED.0
    );
    const_assert_eq!(
        SendFlags::SOLICITED.bits(),
        ffi::ibv_send_flags::IBV_SEND_SOLICITED.0
    );
    const_assert_eq!(
        SendFlags::INLINE.bits(),
        ffi::ibv_send_flags::IBV_SEND_INLINE.0
    );

    use phoenix_api::net::AccessFlags;
    const_assert_eq!(
        AccessFlags::LOCAL_WRITE.bits(),
        ffi::ibv_access_flags::IBV_ACCESS_LOCAL_WRITE.0
    );
    const_assert_eq!(
        AccessFlags::REMOTE_WRITE.bits(),
        ffi::ibv_access_flags::IBV_ACCESS_REMOTE_WRITE.0
    );
    const_assert_eq!(
        AccessFlags::REMOTE_READ.bits(),
        ffi::ibv_access_flags::IBV_ACCESS_REMOTE_READ.0
    );
    const_assert_eq!(
        AccessFlags::REMOTE_ATOMIC.bits(),
        ffi::ibv_access_flags::IBV_ACCESS_REMOTE_ATOMIC.0
    );
}

impl From<phoenix_api::addrinfo::PortSpace> for rdmacm::PortSpace {
    fn from(other: phoenix_api::addrinfo::PortSpace) -> Self {
        use phoenix_api::addrinfo::PortSpace;
        let inner = match other {
            PortSpace::IPOIB => ffi::rdma_port_space::RDMA_PS_IPOIB,
            PortSpace::TCP => ffi::rdma_port_space::RDMA_PS_TCP,
            PortSpace::UDP => ffi::rdma_port_space::RDMA_PS_UDP,
            PortSpace::IB => ffi::rdma_port_space::RDMA_PS_IB,
        };
        rdmacm::PortSpace(inner)
    }
}

impl From<phoenix_api::addrinfo::AddrInfoHints> for rdmacm::AddrInfoHints {
    fn from(h: phoenix_api::addrinfo::AddrInfoHints) -> Self {
        use phoenix_api::addrinfo::AddrFamily;
        use phoenix_api::net::QpType;
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

        let port_space: rdmacm::PortSpace = h.port_space.into();

        rdmacm::AddrInfoHints {
            flags: flags as _,
            family: family as _,
            qp_type: qp_type as _,
            port_space: port_space.0 as _,
        }
    }
}

impl From<rdmacm::AddrInfo> for phoenix_api::addrinfo::AddrInfo {
    fn from(other: rdmacm::AddrInfo) -> Self {
        use phoenix_api::addrinfo::AddrFamily;
        use phoenix_api::addrinfo::AddrInfoFlags;
        use phoenix_api::addrinfo::PortSpace;
        use phoenix_api::net::QpType;

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
        phoenix_api::addrinfo::AddrInfo {
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

impl From<phoenix_api::addrinfo::AddrInfo> for rdmacm::AddrInfo {
    fn from(other: phoenix_api::addrinfo::AddrInfo) -> Self {
        let hints = phoenix_api::addrinfo::AddrInfoHints::new(
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

impl From<phoenix_api::net::QpCapability> for ibv::QpCapability {
    fn from(other: phoenix_api::net::QpCapability) -> Self {
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

impl From<phoenix_api::net::QpType> for ibv::QpType {
    fn from(other: phoenix_api::net::QpType) -> Self {
        use phoenix_api::net::QpType::*;
        let inner = match other {
            RC => ffi::ibv_qp_type::IBV_QPT_RC,
            UD => ffi::ibv_qp_type::IBV_QPT_UD,
        };
        ibv::QpType(inner)
    }
}

impl From<ffi::ibv_wc> for phoenix_api::net::WorkCompletion {
    fn from(other: ffi::ibv_wc) -> Self {
        use ffi::ibv_wc_opcode;
        use ffi::ibv_wc_status;
        use phoenix_api::net::WcOpcode;
        use phoenix_api::net::WcStatus;
        let status = match other.status {
            ibv_wc_status::IBV_WC_SUCCESS => WcStatus::Success,
            e => WcStatus::Error(NonZeroU32::new(e).unwrap()),
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
                code => panic!("unimplemented opcode: {:?}, wc: {:?}", code, other),
            }
        } else {
            WcOpcode::Invalid
        };

        let wc_flags = phoenix_api::net::WcFlags::from_bits(other.wc_flags.0).unwrap();

        phoenix_api::net::WorkCompletion {
            wr_id: other.wr_id,
            status,
            opcode,
            vendor_err: other.vendor_err,
            byte_len: other.byte_len,
            imm_data: other.imm_data,
            qp_num: other.qp_num,
            ud_src_qp: other.qp_num,
            wc_flags,
            // not very relevant
            pkey_index: other.pkey_index,
            slid: other.slid,
            sl: other.sl,
            dlid_path_bits: other.dlid_path_bits,
        }
    }
}

impl From<phoenix_api::net::SendFlags> for ibv::SendFlags {
    fn from(other: phoenix_api::net::SendFlags) -> Self {
        ibv::SendFlags(ffi::ibv_send_flags(other.bits()))
    }
}

impl From<phoenix_api::net::AccessFlags> for ibv::AccessFlags {
    fn from(other: phoenix_api::net::AccessFlags) -> Self {
        ibv::AccessFlags(ffi::ibv_access_flags(other.bits()))
    }
}
