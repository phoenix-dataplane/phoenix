use fnv::FnvHashMap as HashMap;
use std::collections::hash_map;
use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use interface::Handle;

use crate::transport::{DatapathError, Error};

#[derive(Debug)]
pub(crate) struct ResourceTable<R> {
    table: spin::Mutex<HashMap<Handle, Entry<R>>>,
}

impl<R> Default for ResourceTable<R> {
    fn default() -> Self {
        ResourceTable {
            table: spin::Mutex::new(HashMap::default()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Entry<R> {
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
    fn data(&self) -> Arc<R> {
        Arc::clone(&self.data)
    }

    /// `Open` means to increment the reference count.
    #[inline]
    fn open(&self) {
        self.refcnt.fetch_add(1, Ordering::AcqRel);
    }

    /// Returns true if the resource has no more references to it.
    #[inline]
    fn close(&self) -> bool {
        self.refcnt.fetch_sub(1, Ordering::AcqRel) == 1
    }
}

impl<R> ResourceTable<R> {
    pub(crate) fn insert(&self, h: Handle, r: R) -> Result<(), Error> {
        match self.table.lock().insert(h, Entry::new(r, 1)) {
            Some(_) => Err(Error::Exists),
            None => Ok(()),
        }
    }

    pub(crate) fn get(&self, h: &Handle) -> Result<Arc<R>, Error> {
        self.table
            .lock()
            .get(h)
            .map(|r| r.data())
            .ok_or(Error::NotFound)
    }

    pub(crate) fn get_dp(&self, h: &Handle) -> Result<Arc<R>, DatapathError> {
        self.table
            .lock()
            .get(h)
            .map(|r| r.data())
            .ok_or(DatapathError::NotFound)
    }

    /// Occupy en entry by only inserting a value but not incrementing the reference count.
    pub(crate) fn occupy_or_create_resource(&self, h: Handle, r: R) {
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

    pub(crate) fn open_or_create_resource(&self, h: Handle, r: R) {
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

    pub(crate) fn open_resource(&self, h: &Handle) -> Result<(), Error> {
        // increase the refcnt
        self.table
            .lock()
            .get(h)
            .map(|r| r.open())
            .ok_or(Error::NotFound)
    }

    pub(crate) fn close_resource(&self, h: &Handle) -> Result<(), Error> {
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
