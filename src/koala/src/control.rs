//! A Control is the entry of control plane. It directs commands from the external
//! world to corresponding module.
use nix::unistd::Pid;
use std::fs;
use std::io;
use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use log::{debug, warn};

use engine::Engine;
use engine::manager::RuntimeManager;
use interface::engine::{EngineType, SchedulingMode};
use ipc::unix::DomainSocket;
use ipc::ChannelFlavor;

use crate::config::Config;
use crate::node::Node;
use crate::{
    mrpc, rpc_adapter,
    transport::{rdma, tcp},
};

pub struct Control {
    sock: DomainSocket,
    config: Config,
    rdma_transport: rdma::module::TransportModule,
    tcp_transport: tcp::module::TransportModule,
    mrpc: mrpc::module::MrpcModule,
}

impl Control {
    pub fn new(runtime_manager: Arc<RuntimeManager>, config: Config) -> Self {
        let koala_path = config.control.prefix.join(&config.control.path);
        if koala_path.exists() {
            fs::remove_file(&koala_path).expect("remove_file");
        }

        let sock = DomainSocket::bind(&koala_path)
            .unwrap_or_else(|e| panic!("Cannot bind domain socket: {}", e));

        sock.set_read_timeout(Some(Duration::from_millis(1)))
            .expect("set_read_timeout");
        sock.set_write_timeout(Some(Duration::from_millis(1)))
            .expect("set_write_timeout");

        Control {
            sock,
            config,
            rdma_transport: rdma::module::TransportModule::new(Arc::clone(&runtime_manager)),
            tcp_transport: tcp::module::TransportModule::new(Arc::clone(&runtime_manager)),
            mrpc: mrpc::module::MrpcModule::new(Arc::clone(&runtime_manager)),
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

    fn build_graph<P: AsRef<Path>>(
        &mut self,
        client_path: P,
        mode: SchedulingMode,
        cred: &UCred,
    ) -> anyhow::Result<()> {
        // create a node for each vertex in the graph
        let mut nodes: Vec<Node> = self
            .config
            .node
            .iter()
            .map(|x| Node::create_from_template(x))
            .collect();
        // build all internal queues
        for e in &self.config.edges.egress {
            assert_eq!(e.len(), 2, "e: {:?}", e);
            let (sender, receiver) = std::sync::mpsc::channel();
            nodes
                .iter_mut()
                .find(|x| x.id == e[0])
                .unwrap()
                .tx_output
                .push(sender);
            nodes
                .iter_mut()
                .find(|x| x.id == e[1])
                .unwrap()
                .tx_input
                .push(receiver);
        }
        for e in &self.config.edges.ingress {
            assert_eq!(e.len(), 2, "e: {:?}", e);
            let (sender, receiver) = std::sync::mpsc::channel();
            nodes
                .iter_mut()
                .find(|x| x.id == e[0])
                .unwrap()
                .rx_output
                .push(sender);
            nodes
                .iter_mut()
                .find(|x| x.id == e[1])
                .unwrap()
                .rx_input
                .push(receiver);
        }
        // build all engines from nodes
        let mut engines: Vec<Box<dyn Engine>> = Vec::new();
        for n in nodes {
            match n.engine_type {
                EngineType::RdmaTransport => panic!(),
                EngineType::TcpTransport => panic!(),
                EngineType::Mrpc => {
                    self.mrpc
                        .handle_new_client(&self.sock, &client_path, mode, cred)?;
                }
                EngineType::RpcAdapter => {
                    // for now, we create the adapter for tcp
                    let client_pid = Pid::from_raw(cred.pid.unwrap());
                    let (service, customer) = ipc::create_channel(ChannelFlavor::Concurrent);
                    let e1 = rpc_adapter::module::RpcAdapterModule::create_engine(
                        service, mode, client_pid,
                    )?;
                    let e2 = self
                        .rdma_transport
                        .create_engine(customer, mode, client_pid)?;
                    engines.push(Box::new(e1));
                    engines.push(Box::new(e2));
                }
                EngineType::Overload => unimplemented!(),
            };
        }
        // submit engines to runtime
        todo!();
        Ok(())
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
                        self.build_graph(client_path, mode, cred)?;
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
