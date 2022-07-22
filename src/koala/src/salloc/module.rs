use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::{EngineType, SchedulingMode};
use ipc;
use ipc::customer::{Customer, ShmCustomer};
use ipc::salloc::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::SallocEngine;
use super::state::{Shared, State};
use crate::config::SallocConfig;
use crate::engine::container::EngineContainer;
use crate::engine::manager::RuntimeManager;
use crate::module::Service;
use crate::node::Node;
use crate::state_mgr::SharedStateManager;

pub(crate) type CustomerType =
    Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct SallocEngineBuilder {
    customer: CustomerType,
    client_pid: Pid,
    _mode: SchedulingMode,
    shared: Arc<Shared>,
}

impl SallocEngineBuilder {
    fn new(
        customer: CustomerType,
        client_pid: Pid,
        mode: SchedulingMode,
        shared: Arc<Shared>,
    ) -> Self {
        SallocEngineBuilder {
            customer,
            client_pid,
            _mode: mode,
            shared,
        }
    }

    fn build(self) -> Result<SallocEngine> {
        // share the state with rpc adapter
        let salloc_state = State::new(self.shared);

        let node = Node::new(EngineType::Salloc);

        Ok(SallocEngine {
            customer: self.customer,
            node,
            indicator: None,
            state: salloc_state,
        })
    }
}

pub struct SallocModule {
    pub(crate) stage_mgr: SharedStateManager<Shared>,
    config: SallocConfig,
    runtime_manager: Arc<RuntimeManager>,
}

impl SallocModule {
    pub(crate) fn new(config: SallocConfig, runtime_manager: Arc<RuntimeManager>) -> Self {
        SallocModule {
            stage_mgr: SharedStateManager::new(),
            config,
            runtime_manager,
        }
    }

    #[allow(unused)]
    pub(crate) fn handle_request(
        &self,
        req: &control_plane::Request,
        _sock: &DomainSocket,
        sender: &SocketAddr,
        _cred: &UCred,
    ) -> Result<()> {
        let _client_path = sender
            .as_pathname()
            .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;
        match req {
            _ => unreachable!("unknown req: {:?}", req),
        }
    }

    pub(crate) fn handle_new_client<P: AsRef<Path>>(
        &mut self,
        sock: &DomainSocket,
        client_path: P,
        mode: SchedulingMode,
        cred: &UCred,
    ) -> Result<()> {
        // 1. generate a path and bind a unix domain socket to it
        let uuid = Uuid::new_v4();
        let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);
        let engine_path = self.config.prefix.join(instance_name);

        // 2. create customer stub
        let customer =
            Customer::from_shm(ShmCustomer::accept(sock, client_path, mode, engine_path)?);

        // 3. the following part are expected to be done in the Engine's constructor.
        // the transport module is responsible for initializing and starting the transport engines
        let client_pid = Pid::from_raw(cred.pid.unwrap());

        let shared = self.stage_mgr.get_or_create(client_pid)?;
        let builder = SallocEngineBuilder::new(customer, client_pid, mode, shared);
        let engine = builder.build()?;

        // 5. submit the engine to a runtime, overwrite the mode, force to use dedicated runtime
        let gid = self
            .runtime_manager
            .get_new_group_id(client_pid, Service(String::from("DEFAULT")));
        self.runtime_manager.submit(
            client_pid,
            gid,
            EngineContainer::new(engine),
            SchedulingMode::Dedicate,
        );

        Ok(())
    }
}
