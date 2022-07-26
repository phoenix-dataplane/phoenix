//! A Control is the entry of control plane. It directs commands from the external
//! world to corresponding module.
use std::fs;
use std::io;
use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail};
use nix::unistd::Pid;

use interface::engine::{EngineType, SchedulingMode};
use ipc::unix::DomainSocket;

use crate::config::Config;
use crate::engine::container::EngineContainer;
use crate::engine::graph::create_channel;
use crate::engine::manager::RuntimeManager;
use crate::engine::upgrade::EngineUpgrader;
use crate::module::NewEngineRequest;
use crate::module::Service;
use crate::node::Node;
use crate::plugin::PluginCollection;
use crate::resource::Error as ResourceError;
use crate::storage::ResourceCollection;
use crate::storage::SharedStorage;
use crate::{
    mrpc, rpc_adapter, salloc,
    transport::{rdma, tcp},
};

pub struct Control {
    sock: DomainSocket,
    config: Config,
    runtime_manager: Arc<RuntimeManager>,
    rdma_transport: rdma::module::TransportModule,
    tcp_transport: tcp::module::TransportModule,
    mrpc: mrpc::module::MrpcModule,
    salloc: salloc::module::SallocModule,
    rpc_adapter: rpc_adapter::module::RpcAdapterModule,
    plugins: Arc<PluginCollection>,
    upgrader: EngineUpgrader,
}

impl Control {
    fn create_service(
        &self,
        service: &Service,
        client_path: &Path,
        mode: SchedulingMode,
        cred: &UCred,
    ) -> anyhow::Result<()> {
        let pid = Pid::from_raw(cred.pid.unwrap());
        if self.upgrader.is_upgrading(pid) {
            eprintln!("upgrading in progress");
            bail!("client {} still upgrading", pid);
        }
        let gid = self.runtime_manager.get_new_group_id(pid, service.clone());
        let engine_types = self
            .plugins
            .service_registry
            .get(service)
            .ok_or(ResourceError::NotFound)?;
        let mut shared = SharedStorage::new();
        let mut global = self
            .runtime_manager
            .global_resource_mgr
            .resource
            .entry(pid)
            .or_insert_with(ResourceCollection::new);


        // crate auxiliary engines in (reverse) topological order
        for aux_engine_type in engine_types.split_last().unwrap().1 {
            let module_name = self.plugins.engine_registry.get(aux_engine_type).unwrap();
            let mut module = self.plugins.modules.get_mut(module_name.value()).unwrap();
            eprintln!("create engine {:?}, module {:?}", aux_engine_type, module.key());
            let request = NewEngineRequest::Auxiliary { pid, mode };
            let engine = module.create_engine(
                aux_engine_type,
                request,
                &mut shared,
                global.value_mut(),
                &&self.plugins.modules,
            )?;
            // submit auxiliary to runtime manager
            if let Some(engine) = engine {
                let container =
                    EngineContainer::new_v2(engine, aux_engine_type.clone(), module.version());
                self.runtime_manager.submit(pid, gid, container, mode);
            }
        }
        
        // finally, create service engine 
        let service_engine_type = engine_types.last().unwrap();
        let module_name = self
            .plugins
            .engine_registry
            .get(service_engine_type)
            .unwrap();
        let mut module = self.plugins.modules.get_mut(module_name.value()).unwrap();
        let request = NewEngineRequest::Service {
            sock: &self.sock,
            client_path,
            mode,
            cred,
        };
        eprintln!("create engine {:?}", service_engine_type);
        let engine = module
            .create_engine(
                service_engine_type,
                request,
                &mut shared,
                global.value_mut(),
                &self.plugins.modules,
            )?
            .ok_or(anyhow!(
                "service engine must always be created, engine_type={:?}",
                service_engine_type
            ))?;
        // Submit service engine to runtime manager
        let container =
            EngineContainer::new_v2(engine, service_engine_type.clone(), module.version());
        self.runtime_manager.submit(pid, gid, container, mode);

        Ok(())
    }

    pub fn new(runtime_manager: Arc<RuntimeManager>, config: Config) -> Self {
        let koala_prefix = &config.control.prefix;
        fs::create_dir_all(koala_prefix)
            .unwrap_or_else(|e| panic!("Failed to create directory for {:?}: {}", koala_prefix, e));

        let koala_path = koala_prefix.join(&config.control.path);
        if koala_path.exists() {
            fs::remove_file(&koala_path).expect("remove_file");
        }

        let sock = DomainSocket::bind(&koala_path)
            .unwrap_or_else(|e| panic!("Cannot bind domain socket at {:?}: {}", koala_path, e));

        sock.set_read_timeout(Some(Duration::from_millis(1)))
            .expect("set_read_timeout");
        sock.set_write_timeout(Some(Duration::from_millis(1)))
            .expect("set_write_timeout");

        // TODO(cjr): make all modules optional
        assert!(config.transport_rdma.is_some());
        let rdma_transport_config = config.transport_rdma.clone().unwrap();
        assert!(config.transport_tcp.is_some());
        let tcp_transport_config = config.transport_tcp.clone().unwrap();
        assert!(config.mrpc.is_some());
        let mrpc_config = config.mrpc.clone().unwrap();
        assert!(config.salloc.is_some());
        let salloc_config = config.salloc.clone().unwrap();

        let plugins = Arc::new(PluginCollection::new());
        log::info!("load initial plugins, plugins={:?}", config.plugin);
        plugins
            .load_or_upgrade_plugins(&config.plugin)
            .expect("failed to load initial plugins");
        eprintln!("all plugins loaded");
        let upgrader = EngineUpgrader::new(Arc::clone(&runtime_manager), Arc::clone(&plugins));
        eprintln!("init upgrader...");

        Control {
            sock,
            config,
            runtime_manager: Arc::clone(&runtime_manager),
            rdma_transport: rdma::module::TransportModule::new(
                rdma_transport_config,
                Arc::clone(&runtime_manager),
            ),
            tcp_transport: tcp::module::TransportModule::new(
                tcp_transport_config,
                Arc::clone(&runtime_manager),
            ),
            mrpc: mrpc::module::MrpcModule::new(mrpc_config, Arc::clone(&runtime_manager)),
            salloc: salloc::module::SallocModule::new(salloc_config, Arc::clone(&runtime_manager)),
            rpc_adapter: rpc_adapter::module::RpcAdapterModule::new(),
            plugins,
            upgrader,
        }
    }

    pub fn mainloop(&mut self) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 65536];
        loop {
            match self.sock.recv_with_credential_from(buf.as_mut_slice()) {
                Ok((size, sender, cred)) => {
                    log::debug!(
                        "received {} bytes from {:?} with credential: {:?}",
                        size,
                        sender,
                        cred
                    );
                    if let Some(cred) = cred {
                        if let Err(e) = self.dispatch(&mut buf[..size], &sender, &cred) {
                            log::warn!("Control dispatch: {}", e);
                        }
                    } else {
                        log::warn!("received data without a credential, ignored");
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => log::warn!("recv failed: {:?}", e),
            }
        }
    }

    fn build_internal_queues(&mut self) -> Vec<Node> {
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
            let (sender, receiver) = create_channel();
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
            let (sender, receiver) = create_channel();
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

        nodes
    }

    fn build_graph<P: AsRef<Path>>(
        &mut self,
        client_path: P,
        mode: SchedulingMode,
        cred: &UCred,
    ) -> anyhow::Result<()> {
        // build internal queues
        let nodes = self.build_internal_queues();

        // build all engines from nodes
        let mut engines: Vec<EngineContainer> = Vec::new();

        // establish a specialized channel between mrpc and rpc-adapter
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut tx = Some(tx);
        let mut rx = Some(rx);
        let (tx2, rx2) = tokio::sync::mpsc::unbounded_channel();
        let mut tx2 = Some(tx2);
        let mut rx2 = Some(rx2);
        for n in nodes {
            match n.engine_type {
                EngineType::RdmaTransport => panic!(),
                EngineType::RdmaConnMgmt => panic!(),
                EngineType::RpcAdapterAcceptor => panic!(),
                EngineType::TcpTransport => panic!(),
                EngineType::Mrpc => {
                    self.mrpc.handle_new_client(
                        &self.sock,
                        &client_path,
                        mode,
                        cred,
                        n,
                        tx.take().unwrap(),
                        rx2.take().unwrap(),
                    )?;
                }
                EngineType::Salloc => {
                    panic!("salloc engine should not appear in the graph, koala will handle it specially");
                }
                EngineType::RpcAdapter => {
                    // for now, we only implement the adapter for rdma
                    let client_pid = Pid::from_raw(cred.pid.unwrap());
                    let e1 = self.rpc_adapter.create_engine(
                        &self.runtime_manager,
                        n,
                        mode,
                        client_pid,
                        rx.take().unwrap(),
                        tx2.take().unwrap(),
                        &mut self.salloc,
                        &mut self.rdma_transport,
                    )?;
                    engines.push(EngineContainer::new(e1));
                }
                EngineType::Overload => unimplemented!(),
                EngineType::SallocV2 => unimplemented!(),
            };
        }
        // submit engines to runtime
        let pid = Pid::from_raw(cred.pid.unwrap());
        let gid = self
            .runtime_manager
            .get_new_group_id(pid, Service(String::from("DEFAULT")));
        for e in engines {
            self.runtime_manager.submit(pid, gid, e, mode);
        }
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
                eprintln!("New client request");
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
                        // self.build_graph(client_path, mode, cred)?;
                        eprintln!("Get request: MRPC");
                        let service = Service(String::from("Mrpc"));
                        self.create_service(&service, client_path, mode, cred)?;
                    }
                    EngineType::Salloc => {
                        let service = Service(String::from("Salloc"));
                        self.create_service(&service, client_path, mode, cred)?;
                        // self.salloc
                        //     .handle_new_client(&self.sock, &client_path, mode, cred)?;
                    }
                    EngineType::SallocV2 => {
                        let service = Service(String::from("Salloc"));
                        self.create_service(&service, client_path, mode, cred)?;
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
            control::Request::Upgrade(descriptors) => {
                log::info!("upgrade koala plugins, plugins={:?}", descriptors);
                let engines_to_upgrade = self.plugins.load_or_upgrade_plugins(&descriptors)?;
                self.upgrader.upgrade(engines_to_upgrade)?;
                Ok(())
            }
        }
    }
}
