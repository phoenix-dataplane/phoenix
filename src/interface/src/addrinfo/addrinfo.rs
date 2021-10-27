use std::ffi::{CStr, CString};
use std::io;
use std::mem;
use std::net::SocketAddr;
use std::ptr;

#[cfg(unix)]
use libc::{getaddrinfo as c_getaddrinfo, freeaddrinfo as c_freeaddrinfo, addrinfo as c_addrinfo};

/// A struct used as the hints argument to getaddrinfo.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AddrInfoHints {
///
pub flags: i32,
///
/// Values are defined by the libc on your system.
pub address: i32,
/// Indicates the type of QP used for communication. Supported types are
/// UD (unreliable datagram) and RC (reliable connected).
pub qp_type: i32,
/// Protcol for this socket. 0 for none. `ai_protocol` in libc.
///
/// Values are defined by the libc on your system.
pub port_space: i32,
}

impl AddrInfoHints {
  /// Create a new AddrInfoHints using built-in types.
  ///
  /// Included Enums only provide common values, for anything else
  /// create this struct directly using appropriate values from the
  /// libc crate.
  #[allow(dead_code)]
  fn new(flags: Option<i32>, address: Option<::AddrFamily>,
         socktype: Option<::SockType>, protocol: Option<::Protocol>)
    -> AddrInfoHints {
    AddrInfoHints {
      flags: flags.unwrap_or(0),
      address: address.map_or(0, |a| a.into()),
      socktype: socktype.map_or(0, |a| a.into()),
      protocol: protocol.map_or(0, |a| a.into()),
    }
  }

  // Create libc addrinfo from AddrInfoHints struct.
  unsafe fn as_addrinfo(&self) -> c_addrinfo {
    let mut addrinfo: c_addrinfo = mem::zeroed();
    addrinfo.ai_flags = self.flags;
    addrinfo.ai_family = self.address;
    addrinfo.ai_socktype = self.socktype;
    addrinfo.ai_protocol = self.protocol;
    addrinfo
  }
}

impl Default for AddrInfoHints {
  /// Generate a blank AddrInfoHints struct, so new values can easily
  /// be specified.
  fn default() -> Self {
    AddrInfoHints {
      flags: 0,
      address: 0,
      socktype: 0,
      protocol: 0,
    }
  }
}

/// Struct that stores socket information, as returned by getaddrinfo.
#[derive(Clone, Debug, PartialEq)]
pub struct AddrInfo {
  /// Optional bitmask arguments, usually set to zero. `ai_flags` in libc.
  pub flags: i32,
  /// Address family for this socket (usually matches protocol family). `ai_family` in libc.
  ///
  /// Values are defined by the libc on your system.
  pub address: i32,
  /// Type of this socket. `ai_socktype` in libc.
  ///
  /// Values are defined by the libc on your system.
  pub socktype: i32,
  /// Protcol family for this socket. `ai_protocol` in libc.
  ///
  /// Values are defined by the libc on your system.
  pub protocol: i32,
  /// Socket address for this socket, usually containing an actual
  /// IP Address and port. Combination of `ai_addrlen` and `ai_addr` in libc.
  pub sockaddr: SocketAddr,
  /// If requested, this is the canonical name for this socket/host. `ai_canonname` in libc.
  pub canonname: Option<String>,
}

impl AddrInfo {
  /// Copy the informataion from the given addrinfo pointer, and
  /// create a new AddrInfo struct with that information.
  ///
  /// Used for interfacing with getaddrinfo.
  unsafe fn from_ptr(a: *mut c_addrinfo) -> io::Result<Self> {
    if a.is_null() {
      return Err(
        io::Error::new(io::ErrorKind::Other,
        "Supplied pointer is null."
      ))?;
    }

    let addrinfo = *a;
    let ((), sockaddr) = SockAddr::init(|storage, len| {
      *len = addrinfo.ai_addrlen as _;
      std::ptr::copy_nonoverlapping(
        addrinfo.ai_addr as *const u8,
        storage as *mut u8,
        addrinfo.ai_addrlen as usize
      );
      Ok(())
    })?;
    let sock = sockaddr.as_socket().ok_or_else(|| io::Error::new(
      io::ErrorKind::Other,
      format!("Found unknown address family: {}", sockaddr.family())
    ))?;
    Ok(AddrInfo {
      flags: 0,
      address: addrinfo.ai_family,
      socktype: addrinfo.ai_socktype,
      protocol: addrinfo.ai_protocol,
      sockaddr: sock,
      canonname: addrinfo.ai_canonname.as_ref().map(|s|
        CStr::from_ptr(s).to_str().unwrap().to_owned()
      ),
    })
  }
}

/// An iterator of `AddrInfo` structs, wrapping a linked-list
/// returned by getaddrinfo.
///
/// It's recommended to use `.collect<io::Result<..>>()` on this
/// to collapse possible errors.
pub struct AddrInfoIter {
  orig: *mut c_addrinfo,
  cur: *mut c_addrinfo,
}

impl Iterator for AddrInfoIter {
  type Item = io::Result<AddrInfo>;

  fn next(&mut self) -> Option<Self::Item> {
    unsafe {
      if self.cur.is_null() { return None; }
      let ret = AddrInfo::from_ptr(self.cur);
      self.cur = (*self.cur).ai_next as *mut c_addrinfo;
      Some(ret)
    }
  }
}

unsafe impl Sync for AddrInfoIter {}
unsafe impl Send for AddrInfoIter {}

impl Drop for AddrInfoIter {
    fn drop(&mut self) {
        unsafe { c_freeaddrinfo(self.orig) }
    }
}