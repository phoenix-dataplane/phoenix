use ratelimit::config::RateLimitConfig;
use ratelimit::module::RateLimitAddon;
use ratelimit::{InitFnResult, PhoenixAddon};

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixAddon>> {
    let config = RateLimitConfig::new(config_string)?;
    let addon = RateLimitAddon::new(config);
    Ok(Box::new(addon))
}
