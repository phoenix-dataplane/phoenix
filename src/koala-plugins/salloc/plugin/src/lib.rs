use std::path::Path;

use salloc::config::SallocConfig;
use salloc::module::SallocModule;
use salloc::KoalaModule;

#[no_mangle]
pub fn init_module(config_path: Option<&Path>) -> Box<dyn KoalaModule> {
    let config = if let Some(path) = config_path {
        SallocConfig::from_path(path).unwrap()
    } else {
        SallocConfig::default()
    };
    let module = SallocModule::new(config);
    Box::new(module)
}
