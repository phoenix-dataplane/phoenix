//! Provide transport independ address translation.
use crate::QpType;
use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Port space
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PortSpace {
    IPOIB,
    TCP,
    UDP,
    IB,
}

/// Address family for the source and destination address.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AddrFamily {
    /// IP protocol family.
    Inet,
    /// IP version 6.
    Inet6,
    /// Infiniband
    Infiniband,
}

bitflags! {
    /// Hint flags that control the operation of getaddrinfo.
    #[derive(Serialize, Deserialize)]
    #[derive(Default)]
    pub struct AddrInfoFlags: u32 {
        /// Indicates that the results will be used on the passive/listening
        /// side of a connection.
        const PASSIVE = 0b00000001;
        /// If specified, then the node parameter, if provided, must be a  numerical
        /// network address.  This flag suppresses any lengthy address resolution.
        const NUMERICHOST = 0b00000010;
        /// If set, this flag suppresses any lengthy route resolution.
        const NOROUTE = 0b00000100;
        /// If set, the ai_family setting should be used as an input hint for inter‐
        /// pretting the node parameter.
        const FAMILY = 0b00001000;
    }
}

/// A structure containing hints about the type of service the caller supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddrInfoHints {
    /// Hint flags that control the operation.
    pub flags: AddrInfoFlags,
    /// Address family for the source and destination address.
    pub family: Option<AddrFamily>,
    /// Indicates the type of QP used for communication.
    pub qp_type: QpType,
    /// Port space in use.
    pub port_space: PortSpace,
}

impl AddrInfoHints {
    pub fn new(
        flags: AddrInfoFlags,
        family: Option<AddrFamily>,
        qp_type: QpType,
        port_space: PortSpace,
    ) -> Self {
        AddrInfoHints {
            flags,
            family,
            qp_type,
            port_space,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddrInfo {
    /// Hint flags that control the operation.
    pub flags: AddrInfoFlags,
    /// Address family for the source and destination address.
    pub family: Option<AddrFamily>,
    /// Indicates the type of QP used for communication.
    pub qp_type: QpType,
    /// Port space in use.
    pub port_space: PortSpace,
    /// The address for the local device.
    pub src_addr: SocketAddr,
    /// The canonical for the source.
    pub src_canonname: Option<String>,
    /// The address for the destination device.
    pub dst_addr: SocketAddr,
    /// The canonical for the destination.
    pub dst_canonname: Option<String>,
}
