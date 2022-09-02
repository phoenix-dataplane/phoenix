use qos::config::QosConfig;
use qos::module::QosAddon;
use qos::{InitFnResult, KoalaAddon};

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaAddon>> {
    let config = QosConfig::new(config_string)?;
    let addon = QosAddon::new(config);
    Ok(Box::new(addon))
}
