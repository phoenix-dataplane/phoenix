//! A Control is the entry of control plane. It directs commands from the external
//! world to corresponding module.
use std::fs;
use std::io;
use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::time::Duration;
use std::sync::Arc;

use log::{debug, warn};

use engine::manager::RuntimeManager;
use ipc::unix::DomainSocket;
use crate::transport::module::TransportModule;

// TODO(cjr): make these configurable, see koala.toml
const KOALA_PATH: &str = "/tmp/koala/koala-control.sock";

pub struct Control {
    sock: DomainSocket,
    transport: TransportModule,
}

impl Control {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        let koala_path = Path::new(KOALA_PATH);
        if koala_path.exists() {
            fs::remove_file(koala_path).expect("remove_file");
        }

        let sock = DomainSocket::bind(koala_path)
            .unwrap_or_else(|e| panic!("Cannot bind domain socket: {}", e));

        sock.set_read_timeout(Some(Duration::from_millis(1)))
            .expect("set_read_timeout");
        sock.set_write_timeout(Some(Duration::from_millis(1)))
            .expect("set_write_timeout");

        Control {
            sock,
            transport: TransportModule::new(Arc::clone(&runtime_manager)),
        }
    }

    pub fn mainloop(&mut self) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 65536];
        loop {
            match self.sock.recv_with_credential_from(buf.as_mut_slice()) {
                Ok((size, sender, cred)) => {
                    debug!(
                        "received {} bytes from {:?} with credential: {:?}",
                        size, sender, cred
                    );
                    if let Some(cred) = cred {
                        if let Err(e) = self.dispatch(&mut buf[..size], &sender, &cred) {
                            warn!("Control dispatch: {}", e);
                        }
                    } else {
                        warn!("received data without a credential, ignored");
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => warn!("recv failed: {:?}", e),
            }
        }
    }

    fn dispatch(
        &mut self,
        buf: &mut [u8],
        sender: &SocketAddr,
        cred: &UCred,
    ) -> anyhow::Result<()> {
        use ipc::control;
        let msg: control::Request = bincode::deserialize(buf).unwrap();
        match msg {
            control::Request::Transport(req) => self.transport.handle_request(&req, &self.sock, sender, cred),
            _ => unreachable!("control::dispatch"),
        }
    }
}
