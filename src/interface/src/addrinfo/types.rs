use libc::c_int;
use libc as c;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum PortSpcae {
    IPOIB,
    TCP,
    UDP,
    IB,
}

/// Address Family
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AddrFamily {
  /// IP protocol family.
  Inet,
  /// IP version 6.
  Inet6,
  /// Infiniband
  Infiniband,
}