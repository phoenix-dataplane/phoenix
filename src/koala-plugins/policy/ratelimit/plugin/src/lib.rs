use std::path::Path;

use ratelimit::config::RateLimitConfig;
use ratelimit::module::RateLimitAddon;
use ratelimit::KoalaAddon;

#[no_mangle]
pub fn init_addon(config_path: Option<&Path>) -> Box<dyn KoalaAddon> {
    let config = if let Some(path) = config_path {
        RateLimitConfig::from_path(path).unwrap()
    } else {
        RateLimitConfig::default()
    };
    let module = RateLimitAddon::new(config);
    Box::new(module)
}
