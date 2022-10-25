use std::alloc::Layout;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use uapi::salloc::cmd;

use super::module::CustomerType;
use super::region::SharedRegion;
use super::state::State as SallocState;
use super::{ControlPathError, ResourceError};

use phoenix::engine::datapath::DataPathNode;
use phoenix::engine::future;
use phoenix::engine::{Decompose, Engine, EngineResult, Indicator};
use phoenix::envelop::ResourceDowncast;
use phoenix::impl_vertex_for_engine;
use phoenix::module::{ModuleCollection, Version};
use phoenix::storage::{ResourceCollection, SharedStorage};
use phoenix::tracing;

pub struct SallocEngine {
    pub(crate) customer: CustomerType,
    pub(crate) indicator: Indicator,
    pub(crate) node: DataPathNode,
    pub(crate) state: SallocState,
}

impl_vertex_for_engine!(SallocEngine, node);

impl Decompose for SallocEngine {
    #[inline]
    fn flush(&mut self) -> Result<()> {
        // NOTE(wyj): nothing need to be done
        // in current implementation
        Ok(())
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        // NOTE(wyj): If we want to upgrade the type of SallocState,
        // we should decompose the states into atomic components
        // e.g., mr_table
        // If state's type needs to be upgraded,
        // all engines that uses SallocState (or Shared)
        // needs to be upgraded at the same time
        // the last engine to detach & unload will decompose the Arc in shared
        // and put the shared resource into global resources (`global`)
        let engine = *self;
        let mut collections = ResourceCollection::with_capacity(2);
        tracing::trace!("dumping Salloc engine states...");
        collections.insert("customer".to_string(), Box::new(engine.customer));
        // NOTE(wyj): to upgrade state, do the following instead
        // if Arc::strong_count(&engine.state.shared) == 1 {
        //     let shared = Arc::try_unwrap(engine.state.shared).unwrap();
        //     collections.insert("shared-pid".to_string(), Box::new(shared.pid));
        //     collections.insert("shared-resource-mr_table".to_string(), Box::new(shared.resource.mr_table));
        // }
        collections.insert("state".to_string(), Box::new(engine.state));
        (collections, engine.node)
    }
}

impl SallocEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        tracing::trace!("restoring Salloc engine");
        let customer = *local
            .remove("customer")
            .unwrap()
            .downcast::<CustomerType>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let state = *local
            .remove("state")
            .unwrap()
            .downcast::<SallocState>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = SallocEngine {
            customer,
            indicator: Default::default(),
            node,
            state,
        };
        Ok(engine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for SallocEngine {
    fn description(self: Pin<&Self>) -> String {
        "SallocEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }
}

impl SallocEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            match self.check_cmd()? {
                Progress(n) => nwork += n,
                Status::Disconnected => return Ok(()),
            }
            self.indicator.set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl SallocEngine {
    fn check_cmd(&mut self) -> Result<Status, ControlPathError> {
        match self.customer.try_recv_cmd() {
            Ok(req) => {
                let result = self.process_cmd(req);
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Err(e) => self.customer.send_comp(cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                Ok(Progress(0))
            }
            Err(ipc::TryRecvError::Disconnected) => Ok(Status::Disconnected),
            Err(ipc::TryRecvError::Other(_e)) => Err(ControlPathError::IpcTryRecv),
        }
    }

    fn process_cmd(&mut self, req: cmd::Command) -> Result<cmd::CompletionKind, ControlPathError> {
        use cmd::Command;
        match req {
            Command::AllocShm(size, align) => {
                // TODO(wyj): implement backend heap allocator to properly handle align
                tracing::trace!("AllocShm, size: {}", size);
                let layout = Layout::from_size_align(size, align)?;
                let region = SharedRegion::new(layout, &self.state.addr_mediator)?;
                // mr's addr on backend side
                let local_addr = region.as_ptr().expose_addr();
                let file_off = 0;

                // send fd
                self.customer.send_fd(&[region.memfd().as_raw_fd()][..])?;

                self.state
                    .resource()
                    .mr_table
                    .lock()
                    .insert(local_addr, region)
                    .map_or_else(|| Ok(()), |_| Err(ResourceError::Exists))?;
                Ok(cmd::CompletionKind::AllocShm(local_addr, file_off))
            }
            Command::DeallocShm(addr) => {
                // TODO(wyj): will shm dealloc when app exits?
                // app may not dealloc all the created shm regions due to lazy_static and potential misbehave
                self.state
                    .resource()
                    .mr_table
                    .lock()
                    .remove(&addr)
                    .map_or_else(|| Err(ResourceError::NotFound), |_| Ok(()))?;
                Ok(cmd::CompletionKind::DeallocShm)
            }
        }
    }
}
