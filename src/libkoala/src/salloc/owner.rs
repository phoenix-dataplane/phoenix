pub trait AllocOwner { }

pub struct AppOwned;
pub struct BackendOwned;

impl AllocOwner for AppOwned {}
impl AllocOwner for BackendOwned {}