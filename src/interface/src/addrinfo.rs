use std::net::SocketAddr;
use serde::{Serialize, Deserialize};

/// Struct that stores socket information, as returned by getaddrinfo.
///
/// This maps to the same definition provided by libc backends.
#[derive(Clone, Debug, PartialEq)]
#[derive(Serialize, Deserialize)]
#[serde(remote = "dns_lookup::AddrInfo")]
struct AddrInfoDef {
  /// Type of this socket.
  ///
  /// Values are defined by the libc on your system.
  pub socktype: i32,
  /// Protcol family for this socket.
  ///
  /// Values are defined by the libc on your system.
  pub protocol: i32,
  /// Address family for this socket (usually matches protocol family).
  ///
  /// Values are defined by the libc on your system.
  pub address: i32,
  /// Socket address for this socket, usually containing an actual
  /// IP Address and port.
  pub sockaddr: SocketAddr,
  /// If requested, this is the canonical name for this socket/host.
  pub canonname: Option<String>,
  /// Optional bitmask arguments, usually set to zero.
  pub flags: i32,
}


#[derive(Debug, Clone, PartialEq)]
#[derive(Serialize, Deserialize)]
pub struct AddrInfo(#[serde(with = "AddrInfoDef")] pub dns_lookup::AddrInfo);

impl From<dns_lookup::AddrInfo> for AddrInfo {
    fn from(ai: dns_lookup::AddrInfo) -> Self {
        AddrInfo(ai)
    }
}