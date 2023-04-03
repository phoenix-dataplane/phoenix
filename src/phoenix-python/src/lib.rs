#![feature(local_key_cell_methods)]
#![feature(strict_provenance)]

use pyo3::prelude::*;


pub mod register;
pub mod allocate;
pub use register::salloc_register;
pub use register::shm_register;
pub use allocate::allocate_shm;
pub use register::Mode;
pub use register::Hint;

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