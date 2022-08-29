use rpc_adapter::module::RpcAdapterModule;
use rpc_adapter::{InitFnResult, KoalaModule};
use rpc_adapter::config::RpcAdapterConfig;

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let config = RpcAdapterConfig::new(config_string)?;
    let module = RpcAdapterModule::new(config);
    Ok(Box::new(module))
}
