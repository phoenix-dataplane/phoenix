use mrpc::config::MrpcConfig;
use mrpc::module::MrpcModule;
use mrpc::{InitFnResult, PhoenixModule};

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = MrpcConfig::new(config_string)?;
    let module = MrpcModule::new(config);
    Ok(Box::new(module))
}
