use scheduler::{InitFnResult, KoalaModule};
use scheduler::module::SchedulerModule;

#[no_mangle]
pub fn init_module(_config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let module = SchedulerModule::new();
    Ok(Box::new(module))
}
