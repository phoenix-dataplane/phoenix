use transport_rdma::config::RdmaTransportConfig;
use transport_rdma::module::RdmaTransportModule;
use transport_rdma::{InitFnResult, KoalaModule};

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
    let config = RdmaTransportConfig::new(config_string)?;
    let module = RdmaTransportModule::new(config);
    Ok(Box::new(module))
}
