use std::alloc::Layout;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;

use futures::future::BoxFuture;

use ipc::salloc::cmd;
use koala::engine::future;
use koala::engine::{Engine, EngineResult, Indicator};

use super::module::CustomerType;
use super::{ControlPathError, ResourceError};

use super::region::SharedRegion;
use super::state::State as SallocState;

pub struct SallocEngine {
    pub(crate) customer: CustomerType,
    pub(crate) indicator: Option<Indicator>,
    pub(crate) state: SallocState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for SallocEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        unsafe { Box::pin(self.get_unchecked_mut().mainloop()) }
    }

    fn description(&self) -> String {
        "SallocEngine".to_owned()
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }
}

koala::unimplemented_ungradable!(SallocEngine);

impl SallocEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            match self.check_cmd()? {
                Progress(n) => nwork += n,
                Status::Disconnected => return Ok(()),
            }
            self.indicator.as_ref().unwrap().set_nwork(nwork);
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
        use cmd::{Command, CompletionKind};
        match req {
            Command::AllocShm(size, align) => {
                // TODO(wyj): implement backend heap allocator to properly handle align
                // tracing::trace!("AllocShm, size: {}", size);
                let layout = Layout::from_size_align(size, align)?;
                let region = SharedRegion::new(layout)?;
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
            Command::NewMappedAddrs(app_vaddrs) => {
                // TODO(wyj): rewrite
                let mut ret = Vec::new();
                for (mr_handle, app_vaddr) in app_vaddrs.iter() {
                    let mr = self.state.resource().recv_mr_table.get(mr_handle)?;
                    ret.push((mr.as_ptr() as usize, *app_vaddr as usize, mr.len()));
                    let mr_local_addr = mr.as_ptr().expose_addr();
                    let mr_remote_mapped = mrpc_marshal::ShmRecvMr {
                        ptr: *app_vaddr,
                        len: mr.len(),
                        // TODO(cjr): update this
                        align: 8 * 1024 * 1024,
                    };
                    self.state
                        .resource()
                        .insert_addr_map(mr_local_addr, mr_remote_mapped)?;
                }
                Ok(CompletionKind::NewMappedAddrs)
            }
        }
    }
}
