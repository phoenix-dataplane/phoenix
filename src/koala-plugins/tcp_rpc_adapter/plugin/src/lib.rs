use tcp_rpc_adapter::module::TcpRpcAdapterModule;
use tcp_rpc_adapter::{InitFnResult, KoalaModule};

#[no_mangle]
pub fn init_module(_config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let module = TcpRpcAdapterModule::new();
    Ok(Box::new(module))
}
