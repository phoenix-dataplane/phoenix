//! IPC on Unix using domain socket.
use std::convert::TryFrom;
use std::io;
use std::mem;
use std::num::TryFromIntError;
use std::os::unix::io::RawFd;
use std::os::unix::net::{AncillaryData, SocketAncillary, UnixDatagram};
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("send_vectored_with_ancillary_to: {0}.")]
    Io(#[from] io::Error),
    #[error("{0}.")]
    TryFromInt(#[from] TryFromIntError),
}

pub fn send_fd<P: AsRef<Path>>(
    sock: &UnixDatagram,
    sock_path: P,
    fds: &[RawFd],
) -> Result<(), Error> {
    let source_len = u32::try_from(fds.len() * mem::size_of::<RawFd>())?;
    let ancilliary_space = unsafe { libc::CMSG_SPACE(source_len) as usize };
    let mut ancillary_buffer = vec![0u8; ancilliary_space];
    let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
    // this should always returns true unless ancilliary_space is computed wrong
    assert!(ancillary.add_fds(fds));

    let bufs = &mut [][..];
    sock.send_vectored_with_ancillary_to(bufs, &mut ancillary, &sock_path)?;
    Ok(())
}

/// Receive a unix file descriptor from a given domain socket.
/// The domain socket must be open.
pub fn recv_fd(sock: &UnixDatagram) -> Result<Vec<RawFd>, Error> {
    let bufs = &mut [][..];
    let mut fds = Vec::new();
    let mut ancillary_buffer = [0; 128];
    let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
    let (_size, truncated) = sock.recv_vectored_with_ancillary(bufs, &mut ancillary)?;
    // TODO(cjr): sanity check the sender, and see if it is the correct koala transport engine

    assert!(!truncated, "TODO: implement the logic to handle more fds");
    for ancillary_result in ancillary.messages() {
        if let AncillaryData::ScmRights(scm_rights) = ancillary_result.unwrap() {
            for fd in scm_rights {
                fds.push(fd);
            }
        }
    }

    Ok(fds)
}
