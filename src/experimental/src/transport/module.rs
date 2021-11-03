use std::fs;
use std::io;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Result;

use ipc::{self, cmd, dp};

use crate::module::Module;
use crate::transport::engine::TransportEngine;
use engine::{manager::RuntimeManager, SchedulingMode};

// TODO(cjr): make this configurable, see koala.toml
const KOALA_PATH: &'static str = "/tmp/koala/koala-transport.sock";

pub struct TransportModule {
    runtime_manager: Arc<RuntimeManager>,
}

impl TransportModule {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        TransportModule { runtime_manager }
    }

    // pub fn with_config() -> Self {
    // }

    fn dispatch(&mut self, sock: &UnixDatagram, buf: &mut [u8], sender: &SocketAddr) -> Result<()> {
        let client_path = sender
            .as_pathname()
            .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;
        let msg: cmd::Request = bincode::deserialize(buf).unwrap();
        match msg {
            cmd::Request::NewClient(mode) => self.handle_new_client(sock, client_path, mode),
            _ => unreachable!(""),
        }
    }

    fn handle_new_client<P: AsRef<Path>>(
        &mut self,
        sock: &UnixDatagram,
        client_path: P,
        mode: SchedulingMode,
    ) -> Result<()> {
        // 1. create an IPC channel with random name
        let (server, server_name) = ipc::OneShotServer::new()?;

        // 2. tell the name to the client
        let mut buf = bincode::serialize(&cmd::Response::NewClient(mode, server_name))?;
        let nbytes = sock.send_to(buf.as_mut_slice(), &client_path)?;
        if nbytes != buf.len() {
            return Err(anyhow!(
                "expect to send {} bytes, but only {} was sent",
                buf.len(),
                nbytes
            ));
        }

        // 3. the client should later connect to the oneshot server, and create these channels
        // to communicate with its transport engine.
        let (_, (cmd_tx, cmd_rx, dp_tx, dp_rx)): (
            _,
            (
                ipc::Sender<cmd::Response>,
                ipc::Receiver<cmd::Request>,
                ipc::Sender<dp::Response>,
                ipc::Receiver<dp::Request>,
            ),
        ) = server.accept()?;

        // 4. the transport module is responsible for initializing and starting the transport engines
        let engine = TransportEngine::new(&client_path, cmd_tx, cmd_rx, dp_tx, dp_rx, mode);
        // submit the engine to a runtime
        self.runtime_manager.submit(Box::new(engine));

        Ok(())
    }
}

impl Module for TransportModule {
    fn bootstrap(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let koala_path = Path::new(KOALA_PATH);
        if koala_path.exists() {
            fs::remove_file(koala_path).expect("remove_file");
        }

        let sock = UnixDatagram::bind(KOALA_PATH)
            .unwrap_or_else(|e| panic!("Cannot bind domain socket: {}", e));

        sock.set_read_timeout(Some(Duration::from_millis(1)))
            .expect("set_read_timeout");
        sock.set_write_timeout(Some(Duration::from_millis(1)))
            .expect("set_write_timeout");

        let mut buf = vec![0u8; 65536];
        loop {
            match sock.recv_from(buf.as_mut_slice()) {
                Ok((size, sender)) => {
                    debug!("received {} bytes from {:?}", size, sender);
                    if let Err(e) = self.dispatch(&sock, &mut buf[..size], &sender) {
                        warn!("{}", e);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => warn!("recv failed: {:?}", e),
            }

            // std::thread::sleep(Duration::from_secs(1));
            // debug!("recv timeout");
        }
    }
}
