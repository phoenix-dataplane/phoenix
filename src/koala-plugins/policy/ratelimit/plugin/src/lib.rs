use ratelimit::config::RateLimitConfig;
use ratelimit::module::RateLimitAddon;
use ratelimit::{InitFnResult, KoalaAddon};

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaAddon>> {
    let config = RateLimitConfig::new(config_string)?;
    let addon = RateLimitAddon::new(config);
    Ok(Box::new(addon))
}
