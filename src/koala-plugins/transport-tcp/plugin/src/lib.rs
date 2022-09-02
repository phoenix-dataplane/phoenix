use transport_tcp::config::TcpTransportConfig;
use transport_tcp::module::TcpTransportModule;
use transport_tcp::{InitFnResult, KoalaModule};

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let config = TcpTransportConfig::new(config_string)?;
    let module = TcpTransportModule::new(config);
    Ok(Box::new(module))
}
