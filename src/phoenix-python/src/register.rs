use pyo3::prelude::*;
use std::path::PathBuf;
use interface::engine::{SchedulingMode,SchedulingHint};
use ipc::service::{Service,ShmService};
use ipc::salloc::dp::{WorkRequestSlot,CompletionSlot};
use ipc::salloc::cmd::{Command,Completion};
use crate::allocate;


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

#[pyfunction]
pub fn salloc_register(
    prefix:String,
    control:String, 
){
    allocate::set(&(prefix,control));
    allocate::SA_CTX.with(|_ctx| {
        // initialize salloc engine
    });
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
        let service : Result<Service<Command, Completion, WorkRequestSlot, CompletionSlot>,ipc::Error> = ShmService::register(
            phoenix_prefix,
            control_path,
            service,
            scheduling_hint,
            config_str,
        );
        if let Ok(_s) = service {
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


