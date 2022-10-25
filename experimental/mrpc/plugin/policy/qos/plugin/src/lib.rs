use qos::config::QosConfig;
use qos::module::QosAddon;
use qos::{InitFnResult, PhoenixAddon};

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixAddon>> {
    let config = QosConfig::new(config_string)?;
    let addon = QosAddon::new(config);
    Ok(Box::new(addon))
}
