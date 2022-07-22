use crate::storage::{ResourceCollection, SharedStorage};

pub trait Unload {
    fn detach(&mut self);
    fn unload(
        self: Box<Self>,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
    ) -> ResourceCollection;
}
