use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use nix::unistd::Pid;
use phoenix::engine::datapath::graph::ChannelDescriptor;
use phoenix::engine::datapath::node::DataPathNode;
use uuid::Uuid;

use ipc::customer::ShmCustomer;
use uapi::engine::SchedulingMode;
use uapi_mrpc::control_plane::Setting;
use uapi_mrpc::control_plane::TransportType;
use uapi_mrpc::{cmd, dp};

use phoenix::engine::datapath::meta_pool::MetaBufferPool;
use phoenix::engine::{EnginePair, EngineType};
use phoenix::log;
use phoenix::module::{
    ModuleCollection, ModuleDowncast, NewEngineRequest, PhoenixModule, Service, ServiceInfo,
    Version,
};
use phoenix::state_mgr::SharedStateManager;
use phoenix::storage::{get_default_prefix, ResourceCollection, SharedStorage};

use crate::config::MrpcConfig;

use super::engine::MrpcEngine;
use super::state::{Shared, State};

pub type CustomerType =
    ShmCustomer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct MrpcEngineBuilder {
    customer: CustomerType,
    _client_pid: Pid,
    mode: SchedulingMode,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
    node: DataPathNode,
    serializer_build_cache: PathBuf,
    shared: Arc<Shared>,
}

impl MrpcEngineBuilder {
    #[allow(clippy::too_many_arguments)]
    fn new(
        customer: CustomerType,
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
        node: DataPathNode,
        serializer_build_cache: PathBuf,
        shared: Arc<Shared>,
    ) -> Self {
        MrpcEngineBuilder {
            customer,
            cmd_tx,
            cmd_rx,
            node,
            _client_pid: client_pid,
            mode,
            serializer_build_cache,
            shared,
        }
    }

    fn build(self) -> Result<MrpcEngine> {
        const META_BUFFER_POOL_CAP: usize = 128;
        const BUF_LEN: usize = 32;

        let state = State::new(self.shared);

        Ok(MrpcEngine {
            _state: state,
            customer: self.customer,
            cmd_tx: self.cmd_tx,
            cmd_rx: self.cmd_rx,
            node: self.node,
            meta_buf_pool: MetaBufferPool::new(META_BUFFER_POOL_CAP),
            _mode: self.mode,
            dispatch_build_cache: self.serializer_build_cache,
            transport_type: None,
            indicator: Default::default(),
            wr_read_buffer: Vec::with_capacity(BUF_LEN),
        })
    }
}

pub struct MrpcModule {
    config: MrpcConfig,
    pub state_mgr: SharedStateManager<Shared>,
}

impl MrpcModule {
    pub const MRPC_ENGINE: EngineType = EngineType("MrpcEngine");
    pub const ENGINES: &'static [EngineType] = &[MrpcModule::MRPC_ENGINE];
    pub const DEPENDENCIES: &'static [EnginePair] =
        &[(MrpcModule::MRPC_ENGINE, EngineType("RpcAdapterEngine"))];

    pub const SERVICE: Service = Service("Mrpc");
    pub const TX_CHANNELS: &'static [ChannelDescriptor] = &[ChannelDescriptor(
        MrpcModule::MRPC_ENGINE,
        EngineType("RpcAdapterEngine"),
        0,
        0,
    )];
    pub const RX_CHANNELS: &'static [ChannelDescriptor] = &[ChannelDescriptor(
        EngineType("RpcAdapterEngine"),
        MrpcModule::MRPC_ENGINE,
        0,
        0,
    )];

    pub const TCP_DEPENDENCIES: &'static [EnginePair] =
        &[(MrpcModule::MRPC_ENGINE, EngineType("TcpRpcAdapterEngine"))];
    pub const TCP_TX_CHANNELS: &'static [ChannelDescriptor] = &[ChannelDescriptor(
        MrpcModule::MRPC_ENGINE,
        EngineType("TcpRpcAdapterEngine"),
        0,
        0,
    )];
    pub const TCP_RX_CHANNELS: &'static [ChannelDescriptor] = &[ChannelDescriptor(
        EngineType("TcpRpcAdapterEngine"),
        MrpcModule::MRPC_ENGINE,
        0,
        0,
    )];
}

impl MrpcModule {
    pub fn new(config: MrpcConfig) -> Self {
        MrpcModule {
            config,
            state_mgr: SharedStateManager::new(),
        }
    }

    // Returns build_cache if it's already an absolute path. Otherwise returns the path relative
    // to the engine's prefix.
    fn get_build_cache_directory(&self, engine_prefix: &PathBuf) -> PathBuf {
        let build_cache = self.config.build_cache.clone();
        if build_cache.is_absolute() {
            build_cache
        } else {
            engine_prefix.join(build_cache)
        }
    }
}

impl PhoenixModule for MrpcModule {
    fn service(&self) -> Option<ServiceInfo> {
        let service = if self.config.transport == TransportType::Tcp {
            let group = vec![Self::MRPC_ENGINE, EngineType("TcpRpcAdapterEngine")];
            ServiceInfo {
                service: MrpcModule::SERVICE,
                engine: MrpcModule::MRPC_ENGINE,
                tx_channels: MrpcModule::TCP_TX_CHANNELS,
                rx_channels: MrpcModule::TCP_RX_CHANNELS,
                scheduling_groups: vec![group],
            }
        } else {
            let group = vec![Self::MRPC_ENGINE, EngineType("RpcAdapterEngine")];
            ServiceInfo {
                service: MrpcModule::SERVICE,
                engine: MrpcModule::MRPC_ENGINE,
                tx_channels: MrpcModule::TX_CHANNELS,
                rx_channels: MrpcModule::RX_CHANNELS,
                scheduling_groups: vec![group],
            }
        };
        Some(service)
    }

    fn engines(&self) -> &[EngineType] {
        MrpcModule::ENGINES
    }

    fn dependencies(&self) -> &[EnginePair] {
        if self.config.transport == TransportType::Tcp {
            MrpcModule::TCP_DEPENDENCIES
        } else {
            MrpcModule::DEPENDENCIES
        }
    }

    fn check_compatibility(&self, _prev: Option<&Version>, _curr: &HashMap<&str, Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> ResourceCollection {
        let module = *self;
        let mut collections = ResourceCollection::new();
        collections.insert("state_mgr".to_string(), Box::new(module.state_mgr));
        collections.insert("config".to_string(), Box::new(module.config));
        collections
    }

    fn migrate(&mut self, prev_module: Box<dyn PhoenixModule>) {
        // NOTE(wyj): we may better call decompose here
        let prev_concrete = unsafe { *prev_module.downcast_unchecked::<Self>() };
        self.state_mgr = prev_concrete.state_mgr;
    }

    fn create_engine(
        &mut self,
        ty: EngineType,
        request: NewEngineRequest,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
    ) -> Result<Option<Box<dyn phoenix::engine::Engine>>> {
        if ty != MrpcModule::MRPC_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }
        if let NewEngineRequest::Service {
            sock,
            client_path,
            mode,
            cred,
            config_string,
        } = request
        {
            // generate a path and bind a unix domain socket to it
            let uuid = Uuid::new_v4();
            let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);

            // use the phoenix_prefix if not otherwise specified
            let phoenix_prefix = get_default_prefix(global)?;
            let engine_prefix = self.config.prefix.as_ref().unwrap_or(phoenix_prefix);
            let engine_path = engine_prefix.join(instance_name);

            // get the directory of build cache
            let build_cache = self.get_build_cache_directory(engine_prefix);

            // create customer stub
            let customer = ShmCustomer::accept(sock, client_path, mode, engine_path)?;

            let client_pid = Pid::from_raw(cred.pid.unwrap());
            let shared_state = self.state_mgr.get_or_create(client_pid)?;

            let setting = if let Some(config_string) = config_string {
                serde_json::from_str(&config_string)?
            } else {
                Setting {
                    transport: self.config.transport,
                    nic_index: self.config.nic_index,
                    core_id: None,
                }
            };
            log::debug!("mRPC service setting: {:?}", setting);

            let engine_type = match setting.transport {
                TransportType::Tcp => EngineType("TcpRpcAdapterEngine"),
                TransportType::Rdma => EngineType("RpcAdapterEngine"),
            };

            // obtain senders/receivers of command queues with RpcAdapterEngine
            // the sender/receiver ends are already created,
            // as the RpcAdapterEngine is built first
            // according to the topological order
            let cmd_tx = shared.command_path.get_sender(&engine_type)?;
            let cmd_rx = shared.command_path.get_receiver(&MrpcModule::MRPC_ENGINE)?;

            let builder = MrpcEngineBuilder::new(
                customer,
                client_pid,
                mode,
                cmd_tx,
                cmd_rx,
                node,
                build_cache,
                shared_state,
                // TODO(cjr): store the setting, not necessary now.
            );
            let engine = builder.build()?;

            Ok(Some(Box::new(engine)))
        } else {
            bail!("invalid request type");
        }
    }

    fn restore_engine(
        &mut self,
        ty: EngineType,
        local: ResourceCollection,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        plugged: &ModuleCollection,
        prev_version: Version,
    ) -> Result<Box<dyn phoenix::engine::Engine>> {
        if ty != MrpcModule::MRPC_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }
        let engine = MrpcEngine::restore(local, shared, global, node, plugged, prev_version)?;
        Ok(Box::new(engine))
    }
}
