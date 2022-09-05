use hotel_acl::config::HotelAclConfig;
use hotel_acl::module::HotelAclAddon;
use hotel_acl::{InitFnResult, KoalaAddon};

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaAddon>> {
    let config = HotelAclConfig::new(config_string)?;
    let addon = HotelAclAddon::new(config);
    Ok(Box::new(addon))
}
