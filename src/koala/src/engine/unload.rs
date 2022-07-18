pub use version::{version, Version};

// dump -> unload
// restore -> reinstate
pub trait Unload {
    fn suspend(&mut self);
    fn unload(self);
}
