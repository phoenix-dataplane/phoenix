use salloc::config::SallocConfig;
use salloc::module::SallocModule;
use salloc::{InitFnResult, KoalaModule};

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let config = SallocConfig::new(config_string)?;
    let module = SallocModule::new(config);
    Ok(Box::new(module))
}
