//! IPC on Unix using domain socket.
use std::io;
use std::os::unix::io::RawFd;
use std::os::unix::net::{AncillaryData, SocketAncillary, UnixDatagram};
use std::path::Path;

pub fn send_fd<P: AsRef<Path>>(
    sock: &UnixDatagram,
    sock_path: P,
    fd: RawFd,
) -> Result<(), io::Error> {
    let mut ancillary_buffer = [0; 128];
    let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
    ancillary.add_fds(&[fd][..]);

    let bufs = &mut [][..];
    sock.send_vectored_with_ancillary_to(bufs, &mut ancillary, &sock_path)?;
    Ok(())
}

/// Receive a unix file descriptor from a given domain socket.
/// The domain socket must be open.
pub fn recv_fd(sock: &UnixDatagram) -> Result<RawFd, io::Error> {
    let bufs = &mut [][..];
    let mut fds = Vec::new();
    let mut ancillary_buffer = [0; 128];
    let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
    let (_size, truncated) = sock.recv_vectored_with_ancillary(bufs, &mut ancillary)?;
    // TODO(cjr): sanity check the sender, and see if it is the correct koala transport engine

    assert!(!truncated);
    for ancillary_result in ancillary.messages() {
        if let AncillaryData::ScmRights(scm_rights) = ancillary_result.unwrap() {
            for fd in scm_rights {
                fds.push(fd);
            }
        }
    }
    assert_eq!(fds.len(), 1);

    Ok(fds[0])
}
