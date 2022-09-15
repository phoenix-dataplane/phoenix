use null::config::NullConfig;
use null::module::NullAddon;
use null::{InitFnResult, KoalaAddon};

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaAddon>> {
    let config = NullConfig::new(config_string)?;
    let addon = NullAddon::new(config);
    Ok(Box::new(addon))
}
