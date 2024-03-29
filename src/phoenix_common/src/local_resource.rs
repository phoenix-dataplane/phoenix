use std::cell::RefCell;
use std::collections::hash_map;
use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fnv::FnvHashMap as HashMap;

use phoenix_api::Handle;

use super::resource::Error;

#[derive(Debug)]
pub struct LocalResourceTableGeneric<K: Eq + std::hash::Hash, R> {
    table: RefCell<HashMap<K, Entry<R>>>,
}

pub type LocalResourceTable<R> = LocalResourceTableGeneric<Handle, R>;

impl<K: Eq + std::hash::Hash, R> Default for LocalResourceTableGeneric<K, R> {
    fn default() -> Self {
        LocalResourceTableGeneric {
            table: RefCell::new(HashMap::default()),
        }
    }
}

// TODO(cjr): maybe remove this Entry, since HashMap already returns a Ref/RefMut.
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

impl<K: Eq + std::hash::Hash, R> LocalResourceTableGeneric<K, R> {
    pub fn inner(&self) -> &RefCell<HashMap<K, Entry<R>>> {
        &self.table
    }

    pub fn insert(&self, h: K, r: R) -> Result<(), Error> {
        match self.table.borrow_mut().insert(h, Entry::new(r, 1)) {
            Some(_) => Err(Error::Exists),
            None => Ok(()),
        }
    }

    pub fn get(&self, h: &K) -> Result<Arc<R>, Error> {
        self.table
            .borrow()
            .get(h)
            .map(|r| r.data())
            .ok_or(Error::NotFound)
    }

    pub fn get_dp(&self, h: &K) -> Result<Arc<R>, Error> {
        self.table
            .borrow()
            .get(h)
            .map(|r| r.data())
            .ok_or(Error::NotFound)
    }

    /// Occupy en entry by only inserting a value but not incrementing the reference count.
    pub fn occupy_or_create_resource(&self, h: K, r: R) {
        match self.table.borrow_mut().entry(h) {
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
        match self.table.borrow_mut().entry(h) {
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
            .borrow()
            .get(h)
            .map(|r| r.open())
            .ok_or(Error::NotFound)
    }

    /// Reduces the reference counter by one. Returns the resource when it is the last instance
    /// in the table.
    pub fn close_resource(&self, h: &K) -> Result<Option<Arc<R>>, Error> {
        let close = self
            .table
            .borrow()
            .get(h)
            .map(|r| r.close())
            .ok_or(Error::NotFound)?;
        if close {
            let r = self.table.borrow_mut().remove(h).unwrap();
            return Ok(Some(r.data()));
        }
        Ok(None)
    }
}
