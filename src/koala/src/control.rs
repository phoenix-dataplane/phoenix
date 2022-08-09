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
use crate::engine::datapath::node::create_datapath_channels;
use crate::engine::manager::RuntimeManager;
use crate::engine::manager::ServiceSubscription;
use crate::engine::upgrade::EngineUpgrader;
use crate::module::NewEngineRequest;
use crate::module::Service;
use crate::plugin::Plugin;
use crate::plugin::PluginCollection;
use crate::storage::ResourceCollection;
use crate::storage::SharedStorage;

pub struct Control {
    sock: DomainSocket,
    config: Config,
    runtime_manager: Arc<RuntimeManager>,
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
            bail!("client {} still upgrading", pid);
        }
        let service_registry = self
            .plugins.
            service_registry
            .get(service)
            .ok_or(anyhow!("service {:?} not found in the registry", service))?;
        let tx_channels = service_registry.tx_channels.clone();
        let rx_channels = service_registry.rx_channels.clone();
        let (mut nodes, graph) = create_datapath_channels(tx_channels, rx_channels)?;
        let subscription = ServiceSubscription {
            service: service.clone(),
            addons: Vec::new(),
            graph,
        };

        let mut shared = SharedStorage::new();
        let mut global = self
            .runtime_manager
            .global_resource_mgr
            .resource
            .entry(pid)
            .or_insert_with(ResourceCollection::new);

        let mut containers_to_submit = Vec::with_capacity(service_registry.engines.len());
        // crate auxiliary engines in (reverse) topological order
        for aux_engine_type in service_registry.engines.split_last().unwrap().1 {
            let plugin = self.plugins.engine_registry.get(aux_engine_type).unwrap();
            let module_name = match plugin.value() {
                Plugin::Module(module) => module, 
                Plugin::Addon(_) => panic!("service auxiliary engine {:?}", service),
            };
            let mut module = self.plugins.modules.get_mut(module_name).unwrap();
            eprintln!(
                "creating engine {:?}, module {:?}...",
                aux_engine_type,
                module.key()
            );
            let node = nodes.remove(aux_engine_type).unwrap();
            let request = NewEngineRequest::Auxiliary { pid, mode };
            let engine = module.create_engine(
                aux_engine_type,
                request,
                &mut shared,
                global.value_mut(),
                node,
                &&self.plugins.modules,
            )?;
            // submit auxiliary to runtime manager
            if let Some(engine) = engine {
                let container =
                    EngineContainer::new(engine, aux_engine_type.clone(), module.version());
                containers_to_submit.push(container);
            }
        }

        // finally, create service engine
        let service_engine_type = service_registry.engines.last().unwrap();
        let plugin = self.plugins.engine_registry.get(service_engine_type).unwrap();
        let module_name = match plugin.value() {
            Plugin::Module(module) => module, 
            Plugin::Addon(_) => panic!("service auxiliary engine {:?}", service),
        };
        let mut module = self.plugins.modules.get_mut(module_name).unwrap();
        let request = NewEngineRequest::Service {
            sock: &self.sock,
            client_path,
            mode,
            cred,
        };
        eprintln!(
            "creating engine {:?}, module {:?}...",
            service_engine_type,
            module.key()
        );
        let node = nodes.remove(service_engine_type).unwrap();
        let engine = module
            .create_engine(
                service_engine_type,
                request,
                &mut shared,
                global.value_mut(),
                node,
                &self.plugins.modules,
            )?
            .ok_or(anyhow!(
                "service engine must always be created, engine_type={:?}",
                service_engine_type
            ))?;
        // Submit service engine to runtime manager
        let container =
            EngineContainer::new(engine, service_engine_type.clone(), module.version());
        containers_to_submit.push(container);
        let gid = self.runtime_manager.new_group(pid, subscription);
        for container in containers_to_submit {
            self.runtime_manager.submit(pid, gid, container, mode);
        }

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
                    EngineType::Mrpc => {
                        let service = Service(String::from("Mrpc"));
                        self.create_service(&service, client_path, mode, cred)?;
                    }
                    EngineType::Salloc => {
                        let service = Service(String::from("Salloc"));
                        self.create_service(&service, client_path, mode, cred)?;
                        //     .handle_new_client(&self.sock, &client_path, mode, cred)?;
                    }
                    _ => unimplemented!(),
                }
                Ok(())
            }
            control::Request::Upgrade(descriptors) => {
                eprintln!("add rate limiter");
                self.upgrader.add_addon();
                // log::info!("upgrade koala plugins, plugins={:?}", descriptors);
                // let engines_to_upgrade = self.plugins.load_or_upgrade_plugins(&descriptors)?;
                // self.upgrader.upgrade(engines_to_upgrade)?;
                Ok(())
            },
            _ => {
                todo!()
            }
        }
    }
}
