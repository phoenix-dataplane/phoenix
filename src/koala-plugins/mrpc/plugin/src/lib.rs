use mrpc::config::MrpcConfig;
use mrpc::module::MrpcModule;
use mrpc::{InitFnResult, KoalaModule};

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let config = MrpcConfig::new(config_string)?;
    let module = MrpcModule::new(config);
    Ok(Box::new(module))
}
