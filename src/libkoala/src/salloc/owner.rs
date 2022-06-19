pub trait AllocOwner {
    fn is_app_owend() -> bool;
}

#[derive(Debug)]
pub struct AppOwned;

#[derive(Debug)]
pub struct BackendOwned;

impl AllocOwner for AppOwned {
    #[inline(always)]
    fn is_app_owend() -> bool {
        true
    }
}
impl AllocOwner for BackendOwned {
    #[inline(always)]
    fn is_app_owend() -> bool {
        false
    }
}
