use std::collections::{HashMap, VecDeque};
use std::os::unix::ucred::UCred;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result};
use koala::engine::datapath::node::DataPathNode;
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::SchedulingMode;
use ipc::customer::{Customer, ShmCustomer};
use ipc::transport::rdma::{cmd, dp};
use ipc::unix::DomainSocket;

use koala::engine::{Engine, EnginePair, EngineType};
use koala::module::{
    KoalaModule, ModuleCollection, ModuleDowncast, NewEngineRequest, Service, Version,
};
use koala::state_mgr::SharedStateManager;
use koala::storage::{ResourceCollection, SharedStorage};

use crate::cm::engine::CmEngine;
use crate::config::RdmaTransportConfig;
use crate::engine::TransportEngine;
use crate::ops::Ops;
use crate::state::{Shared, State};

pub type CustomerType =
    Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct CmEngineBuilder {
    _client_pid: Pid,
    node: DataPathNode,
    shared: Arc<Shared>,
}

impl CmEngineBuilder {
    fn new(shared: Arc<Shared>, client_pid: Pid, node: DataPathNode) -> Self {
        CmEngineBuilder {
            _client_pid: client_pid,
            node,
            shared,
        }
    }

    fn build(self) -> Result<CmEngine> {
        let state = State::new(self.shared);
        let engine = CmEngine::new(state, self.node);
        Ok(engine)
    }
}

pub(crate) struct TransportEngineBuilder {
    customer: CustomerType,
    node: DataPathNode,
    mode: SchedulingMode,
    ops: Ops,
}

impl TransportEngineBuilder {
    fn new(customer: CustomerType, node: DataPathNode, mode: SchedulingMode, ops: Ops) -> Self {
        TransportEngineBuilder {
            customer,
            node,
            mode,
            ops,
        }
    }

    fn build(self) -> Result<TransportEngine> {
        const BUF_LEN: usize = 32;

        Ok(TransportEngine {
            customer: self.customer,
            indicator: None,
            _mode: self.mode,
            node: self.node,
            ops: self.ops,
            cq_err_buffer: VecDeque::new(),
            wr_read_buffer: Vec::with_capacity(BUF_LEN),
        })
    }
}

pub struct RdmaTransportModule {
    config: RdmaTransportConfig,
    pub state_mgr: SharedStateManager<Shared>,
}

impl RdmaTransportModule {
    pub fn new(config: RdmaTransportConfig) -> Self {
        RdmaTransportModule {
            config,
            state_mgr: SharedStateManager::new(),
        }
    }
}

impl KoalaModule for RdmaTransportModule {
    fn service(&self) -> (Service, EngineType) {
        let service = Service(String::from("RdmaTransport"));
        let etype = EngineType(String::from("RdmaTransportEngine"));
        (service, etype)
    }

    fn engines(&self) -> Vec<EngineType> {
        let e1 = EngineType(String::from("RdmaCmEngine"));
        let e2 = EngineType(String::from("RdmaTransportEngine"));
        vec![e1, e2]
    }

    fn dependencies(&self) -> Vec<EnginePair> {
        let p1 = (
            EngineType(String::from("RdmaTransportEngine")),
            EngineType(String::from("RdmaCmEngine")),
        );
        vec![p1]
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

    fn migrate(&mut self, prev_module: Box<dyn KoalaModule>) {
        // NOTE(wyj): we may better call decompose here
        let prev_concrete = unsafe { *prev_module.downcast_unchecked::<Self>() };
        self.state_mgr = prev_concrete.state_mgr;
    }

    fn create_engine(
        &mut self,
        ty: &EngineType,
        request: NewEngineRequest,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
    ) -> Result<Option<Box<dyn Engine>>> {
        match ty.0.as_str() {
            "RdmaCmEngine" => {
                if let NewEngineRequest::Auxiliary {
                    pid: client_pid,
                    mode: _,
                } = request
                {
                    let engine = self.create_cm_engine(client_pid, node)?;
                    let boxed = engine.map(|x| Box::new(x) as _);
                    Ok(boxed)
                } else {
                    bail!("invalid request type")
                }
            }
            "RdmaTransportEngine" => {
                if let NewEngineRequest::Service {
                    sock,
                    client_path,
                    mode,
                    cred,
                } = request
                {
                    let engine =
                        self.create_transport_engine(sock, client_path, mode, node, cred)?;
                    Ok(Some(Box::new(engine)))
                } else {
                    bail!("invalid request type")
                }
            }
            _ => bail!("invalid engine type {:?}", ty),
        }
    }

    fn restore_engine(
        &mut self,
        ty: &EngineType,
        local: ResourceCollection,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        plugged: &ModuleCollection,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>> {
        match ty.0.as_str() {
            "RdmaCmEngine" => {
                let engine = CmEngine::restore(local, shared, global, plugged, node, prev_version)?;
                Ok(Box::new(engine))
            }
            "RdmaTransportEngine" => {
                let engine =
                    TransportEngine::restore(local, shared, global, plugged, node, prev_version)?;
                Ok(Box::new(engine))
            }
            _ => bail!("invalid engine type {:?}", ty),
        }
    }

    fn tx_channels(&self) -> Vec<koala::engine::datapath::graph::ChannelDescriptor> {
        Vec::new()
    }

    fn rx_channels(&self) -> Vec<koala::engine::datapath::graph::ChannelDescriptor> {
        Vec::new()
    }
}

impl RdmaTransportModule {
    fn create_transport_engine(
        &mut self,
        sock: &DomainSocket,
        client_path: &Path,
        mode: SchedulingMode,
        node: DataPathNode,
        cred: &UCred,
    ) -> Result<TransportEngine> {
        let uuid = Uuid::new_v4();
        let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);
        let engine_path = self.config.prefix.join(instance_name);

        let customer =
            Customer::from_shm(ShmCustomer::accept(sock, client_path, mode, engine_path)?);

        // 3. the following part are expected to be done in the Engine's constructor.
        // the transport module is responsible for initializing and starting the transport engines
        let client_pid = Pid::from_raw(cred.pid.unwrap());

        // 3.1. create the ops and cm engine
        let ops = self.create_ops(client_pid)?;

        // 4. create the engine
        let builder = TransportEngineBuilder::new(customer, node, mode, ops);
        let engine = builder.build()?;
        Ok(engine)
    }

    fn create_cm_engine(
        &mut self,
        client_pid: Pid,
        node: DataPathNode,
    ) -> Result<Option<CmEngine>> {
        let shared = self.state_mgr.get_or_create(client_pid)?;

        // only create one cm_engine for a client process
        // if refcnt > 1, then there is already a CmEngine running
        if Arc::strong_count(&shared) > 1 {
            return Ok(None);
        }

        let builder = CmEngineBuilder::new(shared, client_pid, node);
        let engine = builder.build()?;

        Ok(Some(engine))
    }

    pub fn create_ops(&mut self, client_pid: Pid) -> Result<Ops> {
        if !self.state_mgr.contains(client_pid) {
            bail!(
                "CmEngine hasn't been created for client (pid={})",
                client_pid
            );
        }

        let shared = self.state_mgr.get_or_create(client_pid)?;
        let state = State::new(shared);

        Ok(Ops::new(state))
    }
}
