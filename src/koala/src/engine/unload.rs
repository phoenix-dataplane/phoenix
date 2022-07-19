pub use version::{version, Version};

use crate::storage::ResourceCollection;

use super::Engine;

pub trait Unload {
    fn detach(&mut self);
    fn unload(self: Box<Self>) -> ResourceCollection;
}
