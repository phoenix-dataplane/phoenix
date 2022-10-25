use transport_rdma::config::RdmaTransportConfig;
use transport_rdma::module::RdmaTransportModule;
use transport_rdma::{InitFnResult, PhoenixModule};

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = RdmaTransportConfig::new(config_string)?;
    let module = RdmaTransportModule::new(config);
    Ok(Box::new(module))
}
