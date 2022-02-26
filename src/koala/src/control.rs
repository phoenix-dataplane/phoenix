//! A Control is the entry of control plane. It directs commands from the external
//! world to corresponding module.
use std::fs;
use std::io;
use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use log::{debug, warn};

use crate::mrpc::module::MrpcModule;
use crate::transport::{rdma, tcp};

use engine::manager::RuntimeManager;
use interface::engine::EngineType;
use ipc::unix::DomainSocket;

// TODO(cjr): make these configurable, see koala.toml
const KOALA_PATH: &str = "/tmp/koala/koala-control.sock";

pub struct Control {
    sock: DomainSocket,
    rdma_transport: rdma::module::TransportModule,
    tcp_transport: tcp::module::TransportModule,
    mrpc: MrpcModule,
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
            rdma_transport: rdma::module::TransportModule::new(Arc::clone(&runtime_manager)),
            tcp_transport: tcp::module::TransportModule::new(Arc::clone(&runtime_manager)),
            mrpc: MrpcModule::new(Arc::clone(&runtime_manager)),
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
            control::Request::NewClient(mode, engine_type) => {
                let client_path = sender
                    .as_pathname()
                    .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;
                match engine_type {
                    EngineType::RdmaTransport => {
                        self.rdma_transport.handle_new_client(
                            &self.sock,
                            client_path,
                            mode,
                            cred,
                        )?;
                    }
                    EngineType::TcpTransport => {
                        self.tcp_transport.handle_new_client(
                            &self.sock,
                            client_path,
                            mode,
                            cred,
                        )?;
                    }
                    EngineType::Mrpc => {
                        self.mrpc
                            .handle_new_client(&self.sock, client_path, mode, cred)?;
                    }
                    _ => unimplemented!(),
                }
                Ok(())
            }
            control::Request::RdmaTransport(req) => self
                .rdma_transport
                .handle_request(&req, &self.sock, sender, cred),
            control::Request::TcpTransport(req) => self
                .tcp_transport
                .handle_request(&req, &self.sock, sender, cred),
            control::Request::Mrpc(req) => self.mrpc.handle_request(&req, &self.sock, sender, cred),
            _ => unreachable!("control::dispatch"),
        }
    }
}
