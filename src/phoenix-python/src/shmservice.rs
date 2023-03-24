use pyo3::prelude::*;
use std::path::PathBuf;
use interface::engine::{SchedulingMode,SchedulingHint};
use ipc::service::{Service,ShmService};
use ipc::mrpc::dp::{WorkRequestSlot,CompletionSlot};
use ipc::salloc::cmd::{Command,Completion,CompletionKind};
use salloc::backend::Error;
use std::cell::RefCell;
use memfd::Memfd;
use mmap::MmapFixed;
use std::io;

#[pyclass]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Dedicate,
    Compact,
    Spread
}

#[pyclass]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hint {
    pub mode: Mode,
    pub affinity: Option<u8>
}

#[pymethods]
impl Hint {
    #[new]
    fn new(mode: Mode, affinity:Option<u8>) -> Self {
        Hint{mode,affinity}
    }
}

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
pub fn salloc_register(
    prefix:String,
    control:String, 
){
    set(&(prefix,control));
    SA_CTX.with(|_ctx| {
        // initialize salloc engine
    });
}

#[pyfunction]
pub fn allocate_shm(len: usize) -> u32 {
    assert!(len > 0);
    ls.with(|ctx| {
        let align = len;
        let req = Command::AllocShm(len, align);
        print!("send cmd");
        ctx.service.send_cmd(req);
        print!("send cmd done");
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
                            println!("good");
                            Ok(WriteRegion::new(remote_addr, len, align, file_off, memfd).unwrap())
                        }
                        Err(e) => {
                            println!("bad");
                            Err(Error::Interface("AllocShm", e))
                        }
                        otherwise => panic!("Expect AllocShm, found {:?}", otherwise),
                    };
                    if let Ok(region) = result {
                        0
                    } else{
                        1
                    }
                    }
                    else{
                        1
                    }
            } else{
                1
            }
    
        } else{
            1
        }

    })
}


#[pyfunction]
pub fn shm_register(
    prefix:String,
    control:String, 
    service:String,
    hint:Hint, 
    config_str:Option<&str>
) { 

        let phoenix_prefix = &*PathBuf::from(prefix);
        let control_path = &*PathBuf::from(control);
        let scheduling_hint = SchedulingHint{
            mode:mode_to_scheduling_mode(hint.mode),
            numa_node_affinity:hint.affinity
        };
        let _service : Result<Service<Command, Completion, WorkRequestSlot, CompletionSlot>,ipc::Error> = ShmService::register(
            phoenix_prefix,
            control_path,
            service,
            scheduling_hint,
            config_str,
        );
        if let Ok(service) = _service {
            println!("GOOD!");
        } else{
            println!("BAD!");
        }
        
}

fn mode_to_scheduling_mode(mode: Mode) -> SchedulingMode {
    match mode {
        Mode::Dedicate => SchedulingMode::Dedicate,
        Mode::Compact => SchedulingMode::Compact,
        Mode::Spread => SchedulingMode::Spread,
    }
}


