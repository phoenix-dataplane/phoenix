//! A Control is the entry of control plane. It forwards commands from the external
//! world to corresponding module.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail};
use ipc::control::Response;
use ipc::control::ResponseKind;
use itertools::Itertools;
use nix::unistd::Pid;

use ipc::control::ServiceSubscriptionInfo;
use ipc::unix::DomainSocket;
use uapi::engine::{SchedulingHint, SchedulingMode};

use phoenix_common::engine::datapath::{ChannelDescriptor, DataPathNode};
use phoenix_common::engine::EngineType;
use phoenix_common::module::{NewEngineRequest, Service};
use phoenix_common::storage::{ResourceCollection, SharedStorage, PHOENIX_PREFIX_KEY};

use crate::config::Config;
use crate::plugin::{Plugin, PluginName};
use crate::plugin_mgr::PluginManager;
use crate::runtime::graph::create_datapath_channels;
use crate::runtime::manager::{EngineId, ServiceSubscription, SubscriptionId};
use crate::runtime::{EngineContainer, EngineUpgrader, RuntimeManager};
use crate::{log, tracing};

pub struct Control {
    sock: DomainSocket,
    runtime_manager: Arc<RuntimeManager>,
    plugins: Arc<PluginManager>,
    upgrader: EngineUpgrader,
    scheduling_override: HashMap<String, SchedulingMode>,
    config: Config,
}

impl Control {
    fn choose_transport(
        &mut self,
        service: &Service,
        config_string: Option<&String>,
    ) -> anyhow::Result<()> {
        if service == &Service("Mrpc") {
            if let Some(config_string) = config_string {
                use uapi_mrpc::control_plane::{Setting, TransportType};
                let setting: Setting = serde_json::from_str(config_string)?;
                if let Some(mrpc_module) = self.config.modules.iter_mut().find(|x| x.name == "Mrpc")
                {
                    match setting.transport {
                        TransportType::Rdma => {
                            if let Some(c) = mrpc_module.config_string.as_ref() {
                                mrpc_module.config_string = Some(c.replace("Tcp", "Rdma"));
                            }
                        }
                        TransportType::Tcp => {
                            if let Some(c) = mrpc_module.config_string.as_ref() {
                                mrpc_module.config_string = Some(c.replace("Rdma", "Tcp"));
                            }
                        }
                    }
                    self.plugins
                        .load_or_upgrade_modules(&[mrpc_module.clone()])
                        .expect("failed to load modules");
                }
            }
        }
        Ok(())
    }
}

impl Control {
    fn create_service(
        &mut self,
        service: Service,
        client_path: &Path,
        service_mode: SchedulingMode,
        scheduling_hint: SchedulingHint,
        cred: &UCred,
        config_string: Option<String>,
    ) -> anyhow::Result<()> {
        let pid = Pid::from_raw(cred.pid.unwrap());
        if self.upgrader.is_upgrading(pid) {
            bail!("client {} still upgrading", pid);
        }

        // NOTE(cjr): Specially handle here. A complete solution needs a large refactoring.
        // TODO(cjr): Need a complete refactoring.
        self.choose_transport(&service, config_string.as_ref())?;

        let service_registry = self
            .plugins
            .service_registry
            .get(&service)
            .ok_or_else(|| anyhow!("service {:?} not found in the registry", service))?;
        let tx_channels = service_registry.tx_channels.iter().copied();
        let rx_channels = service_registry.rx_channels.iter().copied();
        let (mut nodes, graph) = create_datapath_channels(
            tx_channels,
            rx_channels,
            &service_registry.scheduling_groups,
        )?;

        let subscription = ServiceSubscription {
            service,
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

        // Make PHOENIX_PREFIX available during engine creation.
        global.value_mut().insert(
            PHOENIX_PREFIX_KEY.to_owned(),
            Box::new(self.config.control.prefix.clone()), // Box<PathBuf>
        );

        let mut singleton_id = service_registry.scheduling_groups.size();
        let mut containers_to_submit = HashMap::new();
        // crate auxiliary engines in (reverse) topological order
        for aux_engine_type in service_registry.engines.split_last().unwrap().1 {
            let plugin = self.plugins.engine_registry.get(aux_engine_type).unwrap();
            let (module_name, specified_mode) = match plugin.value() {
                (PluginName::Module(module), mode) => (module, *mode),
                (PluginName::Addon(_), _) => {
                    panic!("service engine {:?} is an addon", aux_engine_type)
                }
            };

            let mut module = self.plugins.modules.get_mut(module_name).unwrap();
            tracing::info!(
                "Created engine {:?} of service {:?} for client pid={:?}",
                aux_engine_type,
                service,
                pid
            );
            let node = nodes
                .remove(aux_engine_type)
                .unwrap_or_else(DataPathNode::new);
            let specified_mode = specified_mode.unwrap_or(service_mode);
            let request = NewEngineRequest::Auxiliary {
                pid,
                mode: specified_mode,
                config_string: config_string.clone(),
            };

            let engine = module.create_engine(
                *aux_engine_type,
                request,
                &mut shared,
                global.value_mut(),
                node,
                &self.plugins.modules,
            )?;

            // submit auxiliary to runtime manager
            if let Some(engine) = engine {
                let container = EngineContainer::new(engine, *aux_engine_type, module.version());
                let representative = service_registry
                    .scheduling_groups
                    .find_representative(*aux_engine_type)
                    .unwrap_or_else(|| {
                        singleton_id += 1;
                        singleton_id - 1
                    });
                let entry = containers_to_submit
                    .entry(representative)
                    .or_insert_with(Vec::new);
                entry.push((container, specified_mode));
            }
        }

        // finally, create service engine
        let service_engine_type = service_registry.engines.last().unwrap();
        let plugin = self
            .plugins
            .engine_registry
            .get(service_engine_type)
            .unwrap();
        let (module_name, specified_mode) = match plugin.value() {
            (PluginName::Module(module), mode) => (module, *mode),
            (PluginName::Addon(_), _) => {
                panic!("service engine {:?} is an addon", service_engine_type)
            }
        };
        let mut module = self.plugins.modules.get_mut(module_name).unwrap();
        let specified_mode = specified_mode.unwrap_or(service_mode);
        let request = NewEngineRequest::Service {
            sock: &self.sock,
            client_path,
            mode: specified_mode,
            cred,
            config_string,
        };

        let node = nodes
            .remove(service_engine_type)
            .unwrap_or_else(DataPathNode::new);

        let engine = match module.create_engine(
            *service_engine_type,
            request,
            &mut shared,
            global.value_mut(),
            node,
            &self.plugins.modules,
        ) {
            Ok(Some(ret)) => ret,
            Ok(None) => bail!(
                "service engine must always be created, engine_type={:?}",
                service_engine_type
            ),
            Err(e) => {
                log::error!(
                    "create_engine failed: engine_type: {:?}, error: {}",
                    service_engine_type,
                    e
                );
                return Err(e);
            }
        };

        tracing::info!(
            "Created engine {:?} of service {:?} for client pid={:?}",
            service_engine_type,
            service,
            pid
        );
        // Submit service engine to runtime manager
        let container = EngineContainer::new(engine, *service_engine_type, module.version());
        let representative = service_registry
            .scheduling_groups
            .find_representative(*service_engine_type)
            .unwrap_or_else(|| {
                singleton_id += 1;
                singleton_id - 1
            });
        let entry = containers_to_submit
            .entry(representative)
            .or_insert_with(Vec::new);
        entry.push((container, specified_mode));

        let mut groups_to_submit = Vec::with_capacity(containers_to_submit.len());
        for (_, containers) in containers_to_submit {
            if !containers.iter().map(|x| x.1).all_equal() {
                tracing::error!(
                    "Containers within a scheduling group has different scheduling modes, service: {:?}",
                    service
                );
            }
            let mode = containers[0].1;
            let raw_containers = containers.into_iter().map(|x| x.0).collect::<Vec<_>>();
            groups_to_submit.push((raw_containers, mode));
        }
        let sid = self.runtime_manager.new_subscription(pid, subscription);
        let engines_count = groups_to_submit.iter().map(|(c, _)| c.len()).sum();
        self.runtime_manager
            .service_subscriptions
            .get_mut(&(pid, sid))
            .unwrap()
            .1 = engines_count;

        for (containers, mode) in groups_to_submit {
            self.runtime_manager
                .submit_group(pid, sid, containers, mode, scheduling_hint);
        }
        Ok(())
    }

    /// Create a `Control` instance.
    pub fn new(runtime_manager: Arc<RuntimeManager>, config: Config) -> Self {
        let config_clone = config.clone();

        // Create phoenix working directory if not existing
        let phoenix_prefix = &config.control.prefix;
        fs::create_dir_all(phoenix_prefix).unwrap_or_else(|e| {
            panic!("Failed to create directory for {:?}: {}", phoenix_prefix, e)
        });

        let phoenix_path = phoenix_prefix.join(&config.control.path);
        if phoenix_path.exists() {
            fs::remove_file(&phoenix_path).expect("remove_file");
        }

        // Create the control plane domain socket
        let sock = DomainSocket::bind(&phoenix_path)
            .unwrap_or_else(|e| panic!("Cannot bind domain socket at {:?}: {}", phoenix_path, e));

        sock.set_read_timeout(Some(Duration::from_millis(1)))
            .expect("set_read_timeout");
        sock.set_write_timeout(Some(Duration::from_millis(1)))
            .expect("set_write_timeout");

        // load all preset static modules and addons
        let plugins = Arc::new(
            PluginManager::new(phoenix_prefix, &config.linker)
                .expect("failed to create PluginManager"),
        );
        plugins
            .load_or_upgrade_modules(&config.modules)
            .expect("failed to load preset modules");

        for addon in config.addons.iter() {
            plugins
                .load_or_upgrade_addon(addon)
                .expect("failed to load preset addons");
        }

        let upgrader = EngineUpgrader::new(Arc::clone(&runtime_manager), Arc::clone(&plugins));
        tracing::info!("Control plane initialized");

        let scheduling_override = config
            .scheduling
            .into_iter()
            .map(|x| (x.service, x.mode.into()))
            .collect();

        Control {
            sock,
            runtime_manager: Arc::clone(&runtime_manager),
            plugins,
            upgrader,
            scheduling_override,
            config: config_clone,
        }
    }

    pub fn mainloop(&mut self, exit_flag: &AtomicBool) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 65536];
        while !exit_flag.load(Ordering::Relaxed) {
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
                Err(e) => {
                    if exit_flag.load(Ordering::Relaxed) {
                        break;
                    }
                    log::warn!("recv failed: {:?}", e)
                }
            }
        }
        log::info!("exiting...");
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
            control::Request::NewClient(hint, service_name, config_str) => {
                let client_path = sender
                    .as_pathname()
                    .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;
                let service = unsafe { transmute_service_from_str(service_name.as_str()) };
                let service = *self
                    .plugins
                    .service_registry
                    .get(&service)
                    .ok_or_else(|| anyhow!("service {:?} not found", sender))?
                    .key();
                let desired_mode = hint.mode;
                let mode_override = self
                    .scheduling_override
                    .get(&service_name)
                    .copied()
                    .unwrap_or(desired_mode);
                self.create_service(service, client_path, mode_override, hint, cred, config_str)?;
                Ok(())
            }
            control::Request::EngineRequest(eid, request) => {
                log::info!("Receive engine request");
                let eid = EngineId(eid);
                match self.runtime_manager.engine_subscriptions.get(&eid) {
                    Some(info) => {
                        let rid = info.rid;
                        let guard = self.runtime_manager.inner.lock().unwrap();
                        guard.runtimes[&rid].submit_engine_request(eid, request, *cred);
                    }
                    None => {
                        bail!("engine eid={:?} not found", eid);
                    }
                }
                Ok(())
            }
            control::Request::Upgrade(request) => {
                log::info!("Receive backend upgrade request from phoenixctl");
                let engines_to_upgrade = self.plugins.load_or_upgrade_modules(&request.plugins)?;
                self.upgrader.upgrade(
                    engines_to_upgrade,
                    request.flush,
                    request.detach_subscription,
                )?;
                Ok(())
            }
            control::Request::ListSubscription => {
                let client_path = sender
                    .as_pathname()
                    .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;

                let mut engine_subscriptions = HashMap::new();
                for engine in self.runtime_manager.engine_subscriptions.iter() {
                    let entry = engine_subscriptions
                        .entry((engine.pid, engine.sid))
                        .or_insert_with(Vec::new);
                    entry.push((engine.key().0, engine.engine_type.0.to_string()));
                }
                let mut subscriptions_info =
                    Vec::with_capacity(self.runtime_manager.service_subscriptions.len());
                for subscription in self.runtime_manager.service_subscriptions.iter() {
                    let pid = subscription.key().0.as_raw();
                    let sid = subscription.key().1 .0;
                    let service = subscription.0.service.0.to_string();
                    let mut addons = Vec::with_capacity(subscription.0.addons.len());
                    for addon in subscription.0.addons.iter() {
                        addons.push(addon.0.to_string());
                    }
                    let engines = engine_subscriptions
                        .remove(&(subscription.key().0, subscription.key().1))
                        .unwrap_or_default();

                    let info = ServiceSubscriptionInfo {
                        pid,
                        sid,
                        engines,
                        service,
                        addons,
                    };
                    subscriptions_info.push(info);
                }
                let response = Response(Ok(ResponseKind::ListSubscription(subscriptions_info)));
                let mut buf = bincode::serialize(&response)?;
                let nbytes = self.sock.send_to(buf.as_mut_slice(), client_path)?;
                assert_eq!(
                    nbytes,
                    buf.len(),
                    "expect to send {} bytes, but only {} was sent",
                    buf.len(),
                    nbytes
                );
                tracing::info!("List subscription request completed");
                Ok(())
            }
            control::Request::AttachAddon(mode, request) => {
                log::info!("Receive attach addon request from phoenixctl");
                let addon_engine =
                    unsafe { transmute_engine_type_from_str(request.addon_engine.as_str()) };
                let addon_engine = *self
                    .plugins
                    .engine_registry
                    .get(&addon_engine)
                    .ok_or_else(|| {
                        anyhow!("Addon engine type {:?} not found", request.addon_engine)
                    })?
                    .key();

                let tx_edges_replacement =
                    self.refactor_channel_descriptors(request.tx_channels_replacements)?;
                let rx_edges_replacement =
                    self.refactor_channel_descriptors(request.rx_channels_replacements)?;
                let mut group = HashSet::with_capacity(request.group.len());
                for engine in request.group {
                    let engine_ty = unsafe { transmute_engine_type_from_str(engine.as_str()) };
                    let engine_ty = *self
                        .plugins
                        .engine_registry
                        .get(&engine_ty)
                        .ok_or_else(|| anyhow!("Engine type {:?} not found", engine))?
                        .key();
                    group.insert(engine_ty);
                }

                let pid = Pid::from_raw(request.pid);
                let gid = SubscriptionId(request.sid);
                let config_string =
                    Plugin::load_config(request.config_path, request.config_string)?;
                self.upgrader.attach_addon(
                    pid,
                    gid,
                    addon_engine,
                    mode,
                    tx_edges_replacement,
                    rx_edges_replacement,
                    group,
                    config_string,
                )?;
                Ok(())
            }
            control::Request::DetachAddon(request) => {
                log::info!("Receive detach addon request from phoenixctl");
                let addon_engine =
                    unsafe { transmute_engine_type_from_str(request.addon_engine.as_str()) };
                let addon_engine = *self
                    .plugins
                    .engine_registry
                    .get(&addon_engine)
                    .ok_or_else(|| {
                        anyhow!("Addon engine type {:?} not found", request.addon_engine)
                    })?
                    .key();

                let tx_edges_replacement =
                    self.refactor_channel_descriptors(request.tx_channels_replacements)?;
                let rx_edges_replacement =
                    self.refactor_channel_descriptors(request.rx_channels_replacements)?;

                let pid = Pid::from_raw(request.pid);
                let gid = SubscriptionId(request.sid);
                self.upgrader.detach_addon(
                    pid,
                    gid,
                    addon_engine,
                    tx_edges_replacement,
                    rx_edges_replacement,
                )?;
                Ok(())
            }
        }
    }

    fn refactor_channel_descriptors(
        &self,
        channels: Vec<(String, String, usize, usize)>,
    ) -> anyhow::Result<Vec<ChannelDescriptor>> {
        let mut edges = Vec::with_capacity(channels.len());
        for (sender, receiver, sender_idx, recevier_idx) in channels.into_iter() {
            let sender_engine = unsafe { transmute_engine_type_from_str(sender.as_str()) };
            let receiver_engine = unsafe { transmute_engine_type_from_str(receiver.as_str()) };
            let sender_engine = *self
                .plugins
                .engine_registry
                .get(&sender_engine)
                .ok_or_else(|| anyhow!("Engine type {:?} not found", sender))?
                .key();
            let receiver_engine = *self
                .plugins
                .engine_registry
                .get(&receiver_engine)
                .ok_or_else(|| anyhow!("Engine type {:?} not found", receiver))?
                .key();
            edges.push(ChannelDescriptor(
                sender_engine,
                receiver_engine,
                sender_idx,
                recevier_idx,
            ));
        }
        Ok(edges)
    }
}

unsafe fn transmute_engine_type_from_str(engine: &str) -> EngineType {
    let bytes = engine.as_bytes();
    let (ptr, len) = (bytes.as_ptr(), bytes.len());
    let transmuted = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap();
    EngineType(transmuted)
}

unsafe fn transmute_service_from_str(service: &str) -> Service {
    let bytes = service.as_bytes();
    let (ptr, len) = (bytes.as_ptr(), bytes.len());
    let transmuted = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap();
    Service(transmuted)
}
