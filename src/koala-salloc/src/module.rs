use std::os::unix::net::{SocketAddr, UCred};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use koala::envelop::ResourceDowncast;
use koala::module::{KoalaModule, NewEngineRequest};
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::{EngineType, SchedulingMode};
use ipc;
use ipc::customer::{Customer, ShmCustomer};
use ipc::salloc::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::SallocEngine;
use super::state::{Shared, State};
use koala::state_mgr::SharedStateManager;

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
    fn name(&self) -> &str {
        const name: &'static str = "Salloc";
        name
    }

    fn service(&self) -> (koala::module::Service, koala::engine::EngineType) {
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

    fn create_engine(
        &self,
        ty: &koala::engine::EngineType,
        request: koala::module::NewEngineRequest,
        // shared: &mut SharedStorage,
        // global: &mut ResourceCollection,
        // plugged: &ModuleCollection
    ) -> Result<Box<dyn koala::engine::Engine>, Box<dyn std::error::Error>> {
        if let NewEngineRequest::Service { sock, cleint_path, mode, cred } = request {
            // 1. generate a path and bind a unix domain socket to it
            let uuid = Uuid::new_v4();
            
            // let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);
            let instance_name = format!("sallocv2-{}.sock", uuid);
            let engine_path = PathBuf::from("/tmp/salloc").join(instance_name);

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
        } else { panic!() }

    }

    fn restore_engine(
        &self,
        ty: &koala::engine::EngineType,
        mut local: koala::storage::ResourceCollection,
        // shared: &mut SharedStorage,
        // global: &mut ResourceCollection,
        // plugged: &ModuleCollection
    ) -> Result<Box<dyn koala::engine::Engine>, Box<dyn std::error::Error>> {
        // 0. generate a path and bind a unix domain socket to it
        let customer = unsafe { *local.remove("customer").unwrap().downcast_unchecked::<CustomerType>() }; 
        let state = unsafe { *local.remove("state").unwrap().downcast_unchecked::<State>() };
        let engine = SallocEngine {
            customer,
            indicator: None,
            state,
        };
        Ok(Box::new(engine))
    }
}

