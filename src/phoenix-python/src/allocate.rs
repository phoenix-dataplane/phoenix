use pyo3::prelude::*;
use std::path::PathBuf;
use interface::engine::SchedulingHint;
use ipc::service::ShmService;
use ipc::salloc::dp::{WorkRequestSlot,CompletionSlot};
use ipc::salloc::cmd::{Command,Completion,CompletionKind};
use salloc::backend::Error;
use std::cell::RefCell;
use memfd::Memfd;
use mmap::MmapFixed;
use std::io;
use pyo3::exceptions::PyException;

#[derive(Debug)]
pub struct WriteRegion {
    mmap: MmapFixed,
    remote_addr: usize,
    align: usize,
    _memfd: Memfd,
}
impl WriteRegion {
    pub fn new(
        remote_addr: usize,
        nbytes: usize,
        align: usize,
        file_off: i64,
        memfd: Memfd,
    ) -> Result<Self, ipc::Error> {

        // Map to the same address as remote_addr, panic if it does not work
        let mmap = MmapFixed::new(remote_addr, nbytes, file_off as i64, memfd.as_file())?;

        Ok(WriteRegion {
            mmap,
            remote_addr,
            align,
            _memfd: memfd,
        })
    }
}
pub fn current_setting() -> (String,String) {
    SETTING.with_borrow(|s| s.clone())
}

pub fn set(setting: &(String,String)) {
    SETTING.with_borrow_mut(|s| *s = setting.clone());
}


thread_local! {
    /// Initialization is dynamically performed on the first call to with within a thread.
    #[doc(hidden)]
    pub(crate) static SETTING: RefCell<(String,String)> = RefCell::new(("/tmp/phoenix_eric".to_string(),"control.sock".to_string()));
    pub static SA_CTX: SAContext = SAContext::register(current_setting()).expect("phoenix salloc register failed");
}

pub struct SAContext {
    pub service:
    ShmService<Command, Completion, WorkRequestSlot, CompletionSlot>,
}

impl SAContext {
    fn register(setting:(String,String)) -> Result<SAContext, ipc::Error> {
        let service = ShmService::register(
            &*PathBuf::from(setting.0),
            &*PathBuf::from(setting.1),
            "Salloc".to_string(),
            SchedulingHint::default(),
            None,
        )?;
        Ok(Self { service })
    }
}


#[pyfunction]
pub fn allocate_shm(len: usize) -> PyResult<String> {
    if len & (len - 1) != 0 {
        return Err(PyException::new_err("Length must be a power of 2"));
    }
    SA_CTX.with(|ctx| {
        let align = len;
        let req = Command::AllocShm(len, align);
        let _send = ctx.service.send_cmd(req);
        let fdsresult = ctx.service.recv_fd();
        if let Ok(fds) = fdsresult {
            assert_eq!(fds.len(), 1);
            let memfd_result = Memfd::try_from_fd(fds[0]).map_err(|_| io::Error::last_os_error());
            if let Ok(memfd) = memfd_result{
                let file_metadata = memfd.as_file().metadata();
                if let Ok(metadata) = file_metadata{
                    let file_len = metadata.len() as usize;
                    assert!(file_len >= len);
        
                    let result = match ctx.service.recv_comp().unwrap().0 {
                        Ok(CompletionKind::AllocShm(remote_addr, file_off)) => {
                            Ok(WriteRegion::new(remote_addr, len, align, file_off, memfd).unwrap())
                        }
                        Err(e) => {
                            Err(Error::Interface("AllocShm", e))
                        }
                        otherwise => panic!("Expect AllocShm, found {:?}", otherwise),
                    };
                    if let Ok(region) = result {
                        Ok(format!("{region:#?}"))
                    } else{
                        Err(PyException::new_err("Write region error"))
                    }
                }
                else{
                    Err(PyException::new_err("File metadata error"))
                }
            } else{
                Err(PyException::new_err("Memory file descriptor error"))
            }
        } else{
            Err(PyException::new_err("File descriptor error"))
        }

    })
}
