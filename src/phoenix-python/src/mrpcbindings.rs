use ipc::mrpc::cmd::{Command};
use pyo3::prelude::*;
use ipc::mrpc::control_plane::TransportType;
use interface::Handle;
use std::collections::HashMap;

#[pyclass]
#[derive(Debug, Clone)]
pub enum PythonCommand {
    SetTransport,
    Connect,
    Bind,
    NewMappedAddrs,
    UpdateProtos,
    UpdateProtosInner,
}

pub fn command_to_python_command(command:Command) -> (PythonCommand,PyObject){
    let gil = Python::acquire_gil();
    let py = gil.python();
    match command {
        Command::SetTransport(s) => {
            match s {
                TransportType::Rdma => (PythonCommand::SetTransport, "Rdma".to_object(py)),
                TransportType::Tcp => (PythonCommand::SetTransport, "Tcp".to_object(py)),
            }
        },
        Command::Connect(s) => (PythonCommand::Connect, s.to_string().to_object(py)),
        Command::Bind(s) => (PythonCommand::Bind, s.to_string().to_object(py)),
        Command::NewMappedAddrs(Handle(h),vec) => {
            let vec : Vec<(u64,usize)> = vec.iter().map(|(Handle(hh),s)| (*hh,*s)).collect();
            let mut map = HashMap::new();
            map.insert(h,vec);
            (PythonCommand::NewMappedAddrs, map.to_object(py))
        },
        Command::UpdateProtos(vec) => (PythonCommand::SetTransport, vec.to_object(py)),
        Command::UpdateProtosInner(s) => (PythonCommand::SetTransport, s.to_str().to_object(py))

    }
}