use std::path::Path;

use transport_rdma::config::RdmaTransportConfig;
use transport_rdma::module::RdmaTransportModule;
use transport_rdma::KoalaModule;

#[no_mangle]
pub fn init_module(config_path: Option<&Path>) -> Box<dyn KoalaModule> {
    let config = if let Some(path) = config_path {
        RdmaTransportConfig::from_path(path).unwrap()
    } else {
        RdmaTransportConfig::default()
    };
    let module = RdmaTransportModule::new(config);
    Box::new(module)
}
