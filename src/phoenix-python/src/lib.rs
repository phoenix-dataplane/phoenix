#![feature(local_key_cell_methods)]

use pyo3::prelude::*;

pub mod mrpcbindings;


pub mod shmservice;
pub use shmservice::salloc_register;
pub use shmservice::shm_register;
pub use shmservice::allocate_shm;
pub use shmservice::Mode;
pub use shmservice::Hint;

#[pymodule]
fn phoenix_python(py: Python, m: &PyModule) -> PyResult<()> {
    register_shmservice_module(py, m)?;
    Ok(())
}

fn register_shmservice_module(py: Python<'_>, parent_module: &PyModule) -> PyResult<()> {
    let shmservice_module = PyModule::new(py, "shmservice")?;
    shmservice_module.add_function(wrap_pyfunction!(allocate_shm, shmservice_module)?)?;
    shmservice_module.add_function(wrap_pyfunction!(salloc_register, shmservice_module)?)?;
    shmservice_module.add_function(wrap_pyfunction!(shm_register, shmservice_module)?)?;
    shmservice_module.add_class::<Mode>()?;
    shmservice_module.add_class::<Hint>()?;
    parent_module.add_submodule(shmservice_module)?;
    Ok(())
}