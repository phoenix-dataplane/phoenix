use std::path::Path;

use rpc_adapter::module::RpcAdapterModule;
use rpc_adapter::KoalaModule;

#[no_mangle]
pub fn init_module(_config_path: Option<&Path>) -> Box<dyn KoalaModule> {
    let module = RpcAdapterModule::new();
    Box::new(module)
}
