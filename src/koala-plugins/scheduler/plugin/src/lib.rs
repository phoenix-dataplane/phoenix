use scheduler::module::SchedulerModule;
use scheduler::{InitFnResult, KoalaModule};

#[no_mangle]
pub fn init_module(_config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let module = RpcAdapterModule::new();
    Ok(Box::new(module))
}
