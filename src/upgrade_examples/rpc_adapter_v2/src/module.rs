use anyhow::{anyhow, bail, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use nix::unistd::Pid;

use interface::engine::SchedulingMode;
use ipc;
use ipc::mrpc::cmd;

use mrpc::message::{EngineRxMessage, EngineTxMessage};
use salloc::module::SallocModule;
use salloc::region::AddressMediator;
use salloc::state::Shared as SallocShared;
use salloc::state::State as SallocState;
use transport_rdma::module::RdmaTransportModule;
use transport_rdma::ops::Ops;

use koala::engine::{Engine, EnginePair, EngineType};
use koala::module::{
    KoalaModule, ModuleCollection, ModuleDowncast, NewEngineRequest, Service, Version,
};
use koala::state_mgr::SharedStateManager;
use koala::storage::{ResourceCollection, SharedStorage};

use crate::acceptor::engine::AcceptorEngine;
use crate::ctrl_buf::ControlMsgBuffer;
use crate::engine::RpcAdapterEngine;
use crate::state::{Shared, State};

pub(crate) struct AcceptorEngineBuilder {
    _client_pid: Pid,
    shared: Arc<Shared>,
    ops: Ops,
}

impl AcceptorEngineBuilder {
    fn new(shared: Arc<Shared>, ops: Ops, client_pid: Pid) -> Self {
        AcceptorEngineBuilder {
            _client_pid: client_pid,
            shared,
            ops,
        }
    }

    fn build(self) -> Result<AcceptorEngine> {
        let state = State::new(self.shared);
        let engine = AcceptorEngine::new(state, Box::new(self.ops));
        Ok(engine)
    }
}

pub(crate) struct RpcAdapterEngineBuilder {
    _client_pid: Pid,
    mode: SchedulingMode,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>,
    dp_tx: crossbeam::channel::Sender<EngineRxMessage>,
    dp_rx: crossbeam::channel::Receiver<EngineTxMessage>,
    ops: Ops,
    shared: Arc<Shared>,
    salloc_shared: Arc<SallocShared>,
    addr_mediator: Arc<AddressMediator>,
}

impl RpcAdapterEngineBuilder {
    fn new(
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>,
        dp_tx: crossbeam::channel::Sender<EngineRxMessage>,
        dp_rx: crossbeam::channel::Receiver<EngineTxMessage>,
        ops: Ops,
        shared: Arc<Shared>,
        salloc_shared: Arc<SallocShared>,
        addr_mediator: Arc<AddressMediator>,
    ) -> Self {
        RpcAdapterEngineBuilder {
            _client_pid: client_pid,
            mode,
            cmd_tx,
            cmd_rx,
            dp_tx,
            dp_rx,
            ops,
            shared,
            salloc_shared,
            addr_mediator,
        }
    }

    fn build(self) -> Result<RpcAdapterEngine> {
        let state = State::new(self.shared);
        let salloc_state = SallocState::new(self.salloc_shared, self.addr_mediator);

        Ok(RpcAdapterEngine {
            state,
            odp_mr: None,
            ops: Box::new(self.ops),
            salloc: salloc_state,
            local_buffer: VecDeque::new(),
            cmd_tx: self.cmd_tx,
            cmd_rx: self.cmd_rx,
            dp_tx: self.dp_tx,
            dp_rx: self.dp_rx,
            _mode: self.mode,
            indicator: None,
            recv_mr_usage: fnv::FnvHashMap::default(),
            serialization_engine: None,
            ctrl_buf: ControlMsgBuffer::new(),
            version: 2,
        })
    }
}

pub struct RpcAdapterModule {
    pub state_mgr: SharedStateManager<Shared>,
}

impl RpcAdapterModule {
    pub fn new() -> Self {
        RpcAdapterModule {
            state_mgr: SharedStateManager::new(),
        }
    }
}

impl KoalaModule for RpcAdapterModule {
    fn service(&self) -> (Service, EngineType) {
        // TODO: There is no service for RpcAdapter
        // make this an option
        let service = Service(String::from("RpcAdapter"));
        let etype = EngineType(String::from("RpcAdapterEngine"));
        (service, etype)
    }

    fn engines(&self) -> Vec<EngineType> {
        let e1 = EngineType(String::from("RpcAdapterEngine"));
        let e2 = EngineType(String::from("RpcAcceptorEngine"));
        vec![e1, e2]
    }

    fn dependencies(&self) -> Vec<EnginePair> {
        let p1 = (
            EngineType(String::from("RpcAdapterEngine")),
            EngineType(String::from("RpcAcceptorEngine")),
        );
        let p2 = (
            EngineType(String::from("RpcAcceptorEngine")),
            EngineType(String::from("RdmaCmEngine")),
        );
        let p3 = (
            EngineType(String::from("RpcAdapterEngine")),
            EngineType(String::from("RdmaCmEngine")),
        );
        vec![p1, p2, p3]
    }

    fn check_compatibility(&self, _prev: Option<&Version>, _curr: &HashMap<&str, Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> ResourceCollection {
        let module = *self;
        let mut collections = ResourceCollection::new();
        collections.insert("state_mgr".to_string(), Box::new(module.state_mgr));
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
        shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        plugged: &ModuleCollection,
    ) -> Result<Option<Box<dyn Engine>>> {
        let mut rdma_transport_module = plugged
            .get_mut("RdmaTransport")
            .ok_or(anyhow!("fail to get RdmaTransport module"))?;
        let rdma_transport = rdma_transport_module
            .downcast_mut()
            .ok_or(anyhow!("fail to downcast RdmaTransport module"))?;
        let mut salloc_module = plugged
            .get_mut("Salloc")
            .ok_or(anyhow!("fail to get Salloc module"))?;
        let salloc = salloc_module
            .downcast_mut()
            .ok_or(anyhow!("fail to downcast Salloc module"))?;

        match ty.0.as_str() {
            "RpcAcceptorEngine" => {
                if let NewEngineRequest::Auxiliary {
                    pid: client_pid,
                    mode: _,
                } = request
                {
                    let engine = self.create_acceptor_engine(client_pid, rdma_transport)?;
                    let boxed = engine.map(|x| Box::new(x) as _);
                    Ok(boxed)
                } else {
                    bail!("invalid request type")
                }
            }
            "RpcAdapterEngine" => {
                if let NewEngineRequest::Auxiliary {
                    pid: client_pid,
                    mode,
                } = request
                {
                    let tx_edge = (
                        EngineType("MrpcEngine".to_string()),
                        EngineType("RpcAdapterEngine".to_string()),
                    );
                    let rx_edge = (
                        EngineType("RpcAdapterEngine".to_string()),
                        EngineType("MrpcEngine".to_string()),
                    );
                    let mrpc_ty = tx_edge.0.clone();
                    let rpc_adapter_ty = tx_edge.1.clone();

                    let (dp_tx_edge_sender, dp_tx_edge_receiver) = mrpc::message::create_channel();
                    shared.data_path.put_sender(tx_edge, dp_tx_edge_sender)?;
                    let (dp_rx_edge_sender, dp_rx_edge_receiver) = mrpc::message::create_channel();
                    shared
                        .data_path
                        .put_receiver(rx_edge, dp_rx_edge_receiver)?;

                    let (adapter_cmd_sender, adapter_cmd_receiver) =
                        tokio::sync::mpsc::unbounded_channel();
                    shared
                        .command_path
                        .put_sender(rpc_adapter_ty, adapter_cmd_sender)?;
                    let (mrpc_comp_sender, mrpc_comp_receiver) =
                        tokio::sync::mpsc::unbounded_channel();
                    shared
                        .command_path
                        .put_receiver(mrpc_ty, mrpc_comp_receiver)?;

                    let engine = self.create_rpc_adapter_engine(
                        mode,
                        client_pid,
                        mrpc_comp_sender,
                        adapter_cmd_receiver,
                        dp_rx_edge_sender,
                        dp_tx_edge_receiver,
                        salloc,
                        rdma_transport,
                    )?;
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
        plugged: &ModuleCollection,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>> {
        match ty.0.as_str() {
            "RpcAcceptorEngine" => {
                let engine = unsafe {
                    AcceptorEngine::restore(local, shared, global, plugged, prev_version)?
                };
                Ok(Box::new(engine))
            }
            "RpcAdapterEngine" => {
                let engine = unsafe {
                    RpcAdapterEngine::restore(local, shared, global, plugged, prev_version)?
                };
                Ok(Box::new(engine))
            }
            _ => bail!("invalid engine type {:?}", ty),
        }
    }
}

impl RpcAdapterModule {
    fn create_rpc_adapter_engine(
        &mut self,
        mode: SchedulingMode,
        client_pid: Pid,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Completion>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Command>,
        dp_tx: crossbeam::channel::Sender<EngineRxMessage>,
        dp_rx: crossbeam::channel::Receiver<EngineTxMessage>,
        salloc: &mut SallocModule,
        rdma_transport: &mut RdmaTransportModule,
    ) -> Result<RpcAdapterEngine> {
        // Acceptor engine should already been created at this moment
        let ops = rdma_transport.create_ops(client_pid)?;
        let shared = self.state_mgr.get_or_create(client_pid)?;
        // Get salloc state
        eprintln!(
            "Salloc already created = {}",
            salloc.state_mgr.contains(client_pid)
        );
        let salloc_shared = salloc.state_mgr.get_or_create(client_pid)?;
        let addr_mediator = salloc.get_addr_mediator();

        let builder = RpcAdapterEngineBuilder::new(
            client_pid,
            mode,
            cmd_tx,
            cmd_rx,
            dp_tx,
            dp_rx,
            ops,
            shared,
            salloc_shared,
            addr_mediator,
        );
        let engine = builder.build()?;
        Ok(engine)
    }

    fn create_acceptor_engine(
        &mut self,
        client_pid: Pid,
        rdma_transport: &mut RdmaTransportModule,
    ) -> Result<Option<AcceptorEngine>> {
        let ops = rdma_transport.create_ops(client_pid)?;

        let shared = self.state_mgr.get_or_create(client_pid)?;

        if Arc::strong_count(&shared) > 1 {
            return Ok(None);
        }

        let builder = AcceptorEngineBuilder::new(shared, ops, client_pid);
        let engine = builder.build()?;

        Ok(Some(engine))
    }
}
