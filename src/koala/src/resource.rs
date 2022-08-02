use std::collections::hash_map;
use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// use fnv::FnvHashMap as HashMap;
use std::collections::HashMap;
use thiserror::Error;

use interface::Handle;

#[derive(Error, Debug, Clone)]
pub enum Error {
    #[error("Resource not found in the table")]
    NotFound,
    #[error("Resource exists in the table")]
    Exists,
}

#[derive(Debug)]
pub struct ResourceTableGeneric<K, R> {
    table: spin::Mutex<HashMap<K, Entry<R>>>,
}

pub type ResourceTable<R> = ResourceTableGeneric<Handle, R>;

impl<K, R> Default for ResourceTableGeneric<K, R> {
    fn default() -> Self {
        ResourceTableGeneric {
            table: spin::Mutex::new(HashMap::default()),
        }
    }
}

#[derive(Debug)]
pub struct Entry<R> {
    refcnt: AtomicUsize,
    // NOTE(cjr): either the data held here is Arc, or the resource table takes a closure to modify
    // operates on the resource.
    data: Arc<R>,
}

impl<R> Entry<R> {
    fn new(data: R, refcnt: usize) -> Self {
        Entry {
            refcnt: AtomicUsize::new(refcnt),
            data: Arc::new(data),
        }
    }

    #[inline]
    pub fn data(&self) -> Arc<R> {
        Arc::clone(&self.data)
    }

    /// `Open` means to increment the reference count.
    #[inline]
    pub fn open(&self) {
        self.refcnt.fetch_add(1, Ordering::AcqRel);
    }

    /// Returns true if the resource has no more references to it.
    #[inline]
    pub fn close(&self) -> bool {
        self.refcnt.fetch_sub(1, Ordering::AcqRel) == 1
    }
}

impl<K: Eq + std::hash::Hash, R> ResourceTableGeneric<K, R> {
    pub fn inner(&self) -> &spin::Mutex<HashMap<K, Entry<R>>> {
        &self.table
    }

    pub fn insert(&self, h: K, r: R) -> Result<(), Error> {
        match self.table.lock().insert(h, Entry::new(r, 1)) {
            Some(_) => Err(Error::Exists),
            None => Ok(()),
        }
    }

    pub fn get(&self, h: &K) -> Result<Arc<R>, Error> {
        self.table
            .lock()
            .get(h)
            .map(|r| r.data())
            .ok_or(Error::NotFound)
    }

    pub fn get_dp(&self, h: &K) -> Result<Arc<R>, Error> {
        self.table
            .lock()
            .get(h)
            .map(|r| r.data())
            .ok_or(Error::NotFound)
    }

    /// Occupy en entry by only inserting a value but not incrementing the reference count.
    pub fn occupy_or_create_resource(&self, h: K, r: R) {
        match self.table.lock().entry(h) {
            hash_map::Entry::Occupied(_e) => {
                // TODO(cjr): check if they have the same handle
                mem::forget(r);
            }
            hash_map::Entry::Vacant(e) => {
                e.insert(Entry::new(r, 0));
            }
        }
    }

    pub fn open_or_create_resource(&self, h: K, r: R) {
        match self.table.lock().entry(h) {
            hash_map::Entry::Occupied(mut e) => {
                // TODO(cjr): check if they have the same handle
                e.get_mut().open();
                mem::forget(r);
            }
            hash_map::Entry::Vacant(e) => {
                e.insert(Entry::new(r, 1));
            }
        }
    }

    pub fn open_resource(&self, h: &K) -> Result<(), Error> {
        // increase the refcnt
        self.table
            .lock()
            .get(h)
            .map(|r| r.open())
            .ok_or(Error::NotFound)
    }

    pub fn close_resource(&self, h: &K) -> Result<(), Error> {
        let mut table = self.table.lock();
        let close = table.get(h).map(|r| r.close()).ok_or(Error::NotFound)?;
        if close {
            let r = table.remove(h).unwrap();
            // just to make the drop explicit
            mem::drop(r);
        }
        Ok(())
    }
}
