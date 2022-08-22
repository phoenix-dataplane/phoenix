use std::path::Path;

use mrpc::config::MrpcConfig;
use mrpc::module::MrpcModule;
use mrpc::KoalaModule;

#[no_mangle]
pub fn init_module(config_path: Option<&Path>) -> Box<dyn KoalaModule> {
    let config = if let Some(path) = config_path {
        MrpcConfig::from_path(path).unwrap()
    } else {
        MrpcConfig::default()
    };
    let module = MrpcModule::new(config);
    Box::new(module)
}
