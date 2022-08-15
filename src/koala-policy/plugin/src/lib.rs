use std::path::Path;

use policy::config::RateLimitConfig;
use policy::module::RateLimitAddon;
use policy::Addon;

#[no_mangle]
pub fn init_addon(config_path: Option<&Path>) -> Box<dyn Addon> {
    let config = if let Some(path) = config_path {
        RateLimitConfig::from_path(path).unwrap()
    } else {
        RateLimitConfig::default()
    };
    let module = RateLimitAddon::new(config);
    Box::new(module)
}
