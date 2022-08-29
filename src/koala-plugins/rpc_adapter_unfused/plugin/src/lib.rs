use rpc_adapter::module::RpcAdapterModule;
use rpc_adapter::{InitFnResult, KoalaModule};

#[no_mangle]
pub fn init_module(_config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let module = RpcAdapterModule::new();
    Ok(Box::new(module))
}
