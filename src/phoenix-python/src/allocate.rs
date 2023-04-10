use pyo3::prelude::*;
use std::path::PathBuf;
use interface::engine::SchedulingHint;
use ipc::service::ShmService;
use ipc::salloc::dp::{WorkRequestSlot,CompletionSlot};
use ipc::salloc::cmd::{Command,Completion,CompletionKind};
use std::cell::RefCell;
use memfd::Memfd;
use mmap::MmapFixed;
use std::io;
use pyo3::exceptions::PyException;
use std::num::NonZeroUsize;
use std::ptr::NonNull;
use shm::ptr::ShmPtr;

#[derive(Debug)]
pub struct WriteRegion {
    mmap: MmapFixed,
    _remote_addr: usize,
    _align: usize,
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
            _remote_addr: remote_addr,
            _align: align,
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
    pub(crate) static SETTING: RefCell<(String,String)> = RefCell::new(("/tmp/phoenix_eric_eric".to_string(),"control.sock".to_string()));
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

pub fn as_non_null_ptr(a:NonNull<u8>) -> NonNull<u8> {
    // SAFETY: We know `self` is non-null.
    unsafe { NonNull::new_unchecked(a.as_ptr() as *mut u8) }
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
                            let sr = WriteRegion::new(remote_addr, len, align, file_off, memfd).unwrap();
                            let _addr = sr.mmap.as_ptr().addr();
                            let ptr_app = NonNull::new(sr.mmap.as_mut_ptr()).unwrap();
                            let addr_backend = ptr_app.as_ptr().addr();
                            let ptr_backend = ptr_app.with_addr(NonZeroUsize::new(addr_backend).unwrap());

                            let ptr = unsafe {
                                ShmPtr::new_unchecked(as_non_null_ptr(ptr_app).as_ptr(), as_non_null_ptr(ptr_backend).as_ptr())
                            };

                            Ok(ptr)
                        }
                        Err(_e) => {
                            Err(PyException::new_err("Non AllocShm returned"))
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
