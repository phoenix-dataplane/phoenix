use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::SchedulingMode;
use ipc::customer::{Customer, ShmCustomer};
use ipc::salloc::{cmd, dp};

use koala::engine::EngineType;
use koala::envelop::ResourceDowncast;
use koala::module::{KoalaModule, ModuleDowncast};
use koala::module::{ModuleCollection, NewEngineRequest, Service, Version};
use koala::state_mgr::SharedStateManager;
use koala::storage::{ResourceCollection, SharedStorage};

use super::engine::SallocEngine;
use super::state::{Shared, State};

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

        Ok(SallocEngine {
            customer: self.customer,
            indicator: None,
            state: salloc_state,
        })
    }
}

pub struct SallocModule {
    pub(crate) stage_mgr: SharedStateManager<Shared>,
}

impl KoalaModule for SallocModule {
    fn service(&self) -> (Service, EngineType) {
        let service = koala::module::Service(String::from("Salloc"));
        let etype = koala::engine::EngineType("Salloc".to_string());
        (service, etype)
    }

    fn engines(&self) -> Vec<koala::engine::EngineType> {
        vec![koala::engine::EngineType("Salloc".to_string())]
    }

    fn dependencies(&self) -> Vec<koala::engine::EnginePair> {
        vec![]
    }

    fn check_compatibility(&self, _prev: Option<&Version>, _curr: &HashMap<&str, Version>) -> bool {
        true
    }

    fn migrate(&mut self, prev_module: Box<dyn KoalaModule>) {
        let prev_concrete = unsafe { *prev_module.downcast_unchecked::<Self>() };
        self.stage_mgr = prev_concrete.stage_mgr;
    }

    fn create_engine(
        &mut self,
        ty: &EngineType,
        request: NewEngineRequest,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        _plugged: &ModuleCollection,
    ) -> Result<Box<dyn koala::engine::Engine>> {
        if &ty.0 != "Salloc" {
            bail!("invalid engine type {:?}", ty)
        }
        if let NewEngineRequest::Service {
            sock,
            client_path: cleint_path,
            mode,
            cred,
        } = request
        {
            // 1. generate a path and bind a unix domain socket to it
            let uuid = Uuid::new_v4();

            // let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);
            let instance_name = format!("sallocv2-{}.sock", uuid);
            let engine_path = PathBuf::from("/tmp/koala").join(instance_name);

            // 2. create customer stub
            let customer =
                Customer::from_shm(ShmCustomer::accept(sock, cleint_path, mode, engine_path)?);

            // 3. the following part are expected to be done in the Engine's constructor.
            // the transport module is responsible for initializing and starting the transport engines
            let client_pid = Pid::from_raw(cred.pid.unwrap());

            let shared = self.stage_mgr.get_or_create(client_pid)?;
            let builder = SallocEngineBuilder::new(customer, client_pid, mode, shared);
            let engine = builder.build()?;

            Ok(Box::new(engine))
        } else {
            panic!()
        }
    }

    fn restore_engine(
        &mut self,
        ty: &EngineType,
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Box<dyn koala::engine::Engine>> {
        if &ty.0 != "Salloc" {
            bail!("invalid engine type {:?}", ty)
        }
        tracing::trace!("restoring salloc engine");
        let customer = unsafe {
            *local
                .remove("customer")
                .unwrap()
                .downcast_unchecked::<CustomerType>()
        };
        let state = unsafe { *local.remove("state").unwrap().downcast_unchecked::<State>() };
        let engine = SallocEngine {
            customer,
            indicator: None,
            state,
        };
        Ok(Box::new(engine))
    }
}
