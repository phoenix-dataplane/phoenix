//! IPC on Unix using domain socket.
use std::io;
use std::io::{IoSlice, IoSliceMut};
use std::mem;
use std::num::TryFromIntError;
use std::os::unix::io::RawFd;
use std::os::unix::net::{AncillaryData, SocketAddr, SocketAncillary, SocketCred, UnixDatagram};
use std::os::unix::ucred::UCred;
use std::path::Path;
use std::time;

use thiserror::Error;

const ANCILLARY_BUFFER_SIZE: usize = 8192;

#[derive(Debug, Error)]
pub enum Error {
    #[error("send_vectored_with_ancillary_to: {0}.")]
    Io(#[from] io::Error),
    #[error("{0}.")]
    TryFromInt(#[from] TryFromIntError),
    #[error("bincode: {0}.")]
    Bincode(#[from] bincode::Error),
    #[error("data truncated: {0} bytes sent.")]
    Truncated(usize),
    #[error("peer credential is empty, please make sure the socket is connected")]
    NotConnected,
}

fn get_ucred() -> UCred {
    use nix::unistd::{Gid, Pid, Uid};
    UCred {
        pid: Some(Pid::this().as_raw()),
        uid: Uid::current().as_raw(),
        gid: Gid::current().as_raw(),
    }
}

/// An authenticated domain socket.
#[derive(Debug)]
pub struct DomainSocket {
    sock: UnixDatagram,
    local_cred: UCred,
    // only exists when this socket is not a listener
    peer_cred: Option<UCred>,
}

impl AsRef<UnixDatagram> for DomainSocket {
    fn as_ref(&self) -> &UnixDatagram {
        &self.sock
    }
}

use std::ops::Deref;
impl Deref for DomainSocket {
    type Target = UnixDatagram;
    fn deref(&self) -> &Self::Target {
        &self.sock
    }
}

impl Drop for DomainSocket {
    fn drop(&mut self) {
        if let Ok(local_addr) = self.sock.local_addr() {
            if let Some(path) = local_addr.as_pathname() {
                let _ = std::fs::remove_file(path);
            }
        }
        // sliently ignore the error otherwise in drop function
    }
}

// send_to, recv_from, set_read_timeout, set_write_timeout
impl DomainSocket {
    pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<DomainSocket> {
        let sock = UnixDatagram::bind(&path)?;
        // Enabling this socket option causes receipt of the
        // credentials of the sending process in an SCM_CREDENTIALS
        // ancillary message in each subsequently received message.
        // The returned credentials are those specified by the sender
        // using SCM_CREDENTIALS, or a default that includes the
        // sender's PID, real user ID, and real group ID, if the
        // sender did not specify SCM_CREDENTIALS ancillary data.
        sock.set_passcred(true)?;
        let cred = get_ucred();
        Ok(DomainSocket {
            sock,
            local_cred: cred,
            peer_cred: None,
        })
    }

    pub fn connect<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let path = path.as_ref();
        self.sock.connect(path)?;

        // send local_cred
        let mut buf = vec![0i32; 3];
        buf[0] = self.local_cred.pid.unwrap() as _;
        buf[1] = self.local_cred.uid as _;
        buf[2] = self.local_cred.gid as _;
        let buf2 = bincode::serialize(&buf)?;
        let nbytes = self.send_to(&buf2, path)?;
        if nbytes != buf2.len() {
            return Err(Error::Truncated(nbytes));
        }

        // recv peer_cred
        let mut buf = vec![0u8; 256];
        let (_, sender) = self.sock.recv_from(buf.as_mut_slice())?;
        assert_eq!(sender.as_pathname(), Some(path));

        // set peer_cred
        let buf2: Vec<i32> = bincode::deserialize(&buf)?;
        let peer_cred = UCred {
            pid: Some(buf2[0] as _),
            uid: buf2[1] as _,
            gid: buf2[2] as _,
        };
        self.set_peer_cred(peer_cred);

        Ok(())
    }

    fn set_peer_cred(&mut self, cred: UCred) {
        self.peer_cred = Some(cred);
    }

    pub fn peer_cred(&self) -> Result<UCred, Error> {
        self.peer_cred.ok_or(Error::NotConnected)
    }

    fn add_creds<'a>(&self, ancillary: &mut SocketAncillary<'a>) {
        let mut cred = SocketCred::new();
        // get the real one as is done by default
        cred.set_pid(self.local_cred.pid.expect("Must be sucessful on Linux"));
        cred.set_uid(self.local_cred.uid);
        cred.set_gid(self.local_cred.gid);
        // this should always returns true unless ancilliary_space is computed wrong
        assert!(ancillary.add_creds(&[cred]));
    }

    pub fn send_to<P: AsRef<Path>>(&self, buf: &[u8], path: P) -> io::Result<usize> {
        let source_len = mem::size_of::<SocketCred>() as u32;
        let ancilliary_space = unsafe { libc::CMSG_SPACE(source_len) as usize };
        let mut ancillary_buffer = vec![0u8; ancilliary_space];
        let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
        self.add_creds(&mut ancillary);

        let bufs = &mut [IoSlice::new(buf)][..];
        self.send_vectored_with_ancillary_to(bufs, &mut ancillary, &path)
    }

    pub fn recv_with_credential_from(
        &self,
        buf: &mut [u8],
    ) -> io::Result<(usize, SocketAddr, Option<UCred>)> {
        let bufs = &mut [IoSliceMut::new(buf)][..];
        let mut ancillary_buffer = [0; 256];
        let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
        let (size, truncated, addr) =
            self.recv_vectored_with_ancillary_from(bufs, &mut ancillary)?;
        // TODO(cjr): sanity check the sender, and see if it is the correct phoenix transport engine

        assert!(!truncated, "TODO: implement the logic to handle more fds");

        let mut cred = None;
        for ancillary_result in ancillary.messages() {
            match ancillary_result.unwrap() {
                AncillaryData::ScmRights(_) => panic!("unexpected ScmRights included"),
                AncillaryData::ScmCredentials(scm_creds) => {
                    for sc in scm_creds {
                        assert_eq!(
                            None,
                            cred.replace(UCred {
                                uid: sc.get_uid(),
                                gid: sc.get_gid(),
                                pid: Some(sc.get_pid()),
                            }),
                            "More than one credentials received"
                        );
                    }
                }
            }
        }

        Ok((size, addr, cred))
    }

    pub fn send_fd<P: AsRef<Path>>(&self, sock_path: P, fds: &[RawFd]) -> Result<(), Error> {
        let source_len = u32::try_from(fds.len() * mem::size_of::<RawFd>())?;
        let fd_space = unsafe { libc::CMSG_SPACE(source_len) } as usize;
        let cred_space = unsafe { libc::CMSG_SPACE(mem::size_of::<SocketCred>() as _) } as usize;
        let ancilliary_space = fd_space + cred_space;

        let mut ancillary_buffer = vec![0u8; ancilliary_space];
        let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
        // this should always returns true unless ancilliary_space is computed wrong
        assert!(ancillary.add_fds(fds));
        self.add_creds(&mut ancillary);

        let bufs = &mut [][..];
        self.send_vectored_with_ancillary_to(bufs, &mut ancillary, &sock_path)?;
        Ok(())
    }

    /// Receive a unix file descriptor from a given domain socket.
    /// The domain socket must be open.
    pub fn recv_fd(&self) -> Result<(Vec<RawFd>, Option<UCred>), Error> {
        let bufs = &mut [][..];
        let mut fds = Vec::new();
        let mut ancillary_buffer = [0; ANCILLARY_BUFFER_SIZE];
        let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
        let (_size, truncated) = self.recv_vectored_with_ancillary(bufs, &mut ancillary)?;
        // TODO(cjr): sanity check the sender, and see if it is the correct phoenix transport engine

        assert!(!truncated, "TODO: implement the logic to handle more fds");

        let mut cred = None;
        for ancillary_result in ancillary.messages() {
            match ancillary_result.unwrap() {
                AncillaryData::ScmRights(scm_rights) => {
                    for fd in scm_rights {
                        fds.push(fd);
                    }
                }
                AncillaryData::ScmCredentials(scm_creds) => {
                    for sc in scm_creds {
                        assert_eq!(
                            None,
                            cred.replace(UCred {
                                uid: sc.get_uid(),
                                gid: sc.get_gid(),
                                pid: Some(sc.get_pid()),
                            })
                        );
                    }
                }
            }
        }

        Ok((fds, cred))
    }

    #[allow(clippy::type_complexity)]
    pub fn try_recv_fd(&self) -> Result<Option<(Vec<RawFd>, Option<UCred>)>, Error> {
        self.set_read_timeout(Some(time::Duration::from_micros(1)))?;
        let ret = match self.recv_fd() {
            Ok(ret) => Ok(Some(ret)),
            Err(Error::Io(ref e)) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        };
        // here we may get two errors...
        self.set_read_timeout(None).unwrap();
        ret
    }
}
