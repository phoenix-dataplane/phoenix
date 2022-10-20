use rpc_adapter::config::RpcAdapterConfig;
use rpc_adapter::module::RpcAdapterModule;
use rpc_adapter::{InitFnResult, PhoenixModule};

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = RpcAdapterConfig::new(config_string)?;
    let module = RpcAdapterModule::new(config);
    Ok(Box::new(module))
}
