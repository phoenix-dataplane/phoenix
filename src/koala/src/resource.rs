use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::mapref::entry;
use dashmap::DashMap;
use fnv::FnvBuildHasher;

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
pub(crate) struct ResourceTableGeneric<K: Eq + std::hash::Hash, R> {
    table: DashMap<K, Entry<R>, FnvBuildHasher>,
}

pub(crate) type ResourceTable<R> = ResourceTableGeneric<Handle, R>;

impl<K: Eq + std::hash::Hash, R> Default for ResourceTableGeneric<K, R> {
    fn default() -> Self {
        ResourceTableGeneric {
            table: DashMap::default(),
        }
    }
}

// TODO(cjr): maybe remove this Entry, since DashMap already returns a Ref/RefMut.
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
    pub(crate) fn data(&self) -> Arc<R> {
        Arc::clone(&self.data)
    }

    /// `Open` means to increment the reference count.
    #[inline]
    pub(crate) fn open(&self) {
        self.refcnt.fetch_add(1, Ordering::AcqRel);
    }

    /// Returns true if the resource has no more references to it.
    #[inline]
    pub(crate) fn close(&self) -> bool {
        self.refcnt.fetch_sub(1, Ordering::AcqRel) == 1
    }
}

impl<K: Eq + std::hash::Hash, R> ResourceTableGeneric<K, R> {
    pub(crate) fn inner(&self) -> &DashMap<K, Entry<R>, FnvBuildHasher> {
        &self.table
    }

    pub(crate) fn insert(&self, h: K, r: R) -> Result<(), Error> {
        match self.table.insert(h, Entry::new(r, 1)) {
            Some(_) => Err(Error::Exists),
            None => Ok(()),
        }
    }

    pub(crate) fn get(&self, h: &K) -> Result<Arc<R>, Error> {
        self.table.get(h).map(|r| r.data()).ok_or(Error::NotFound)
    }

    pub(crate) fn get_dp(&self, h: &K) -> Result<Arc<R>, Error> {
        self.table.get(h).map(|r| r.data()).ok_or(Error::NotFound)
    }

    /// Occupy en entry by only inserting a value but not incrementing the reference count.
    pub(crate) fn occupy_or_create_resource(&self, h: K, r: R) {
        match self.table.entry(h) {
            entry::Entry::Occupied(_e) => {
                // TODO(cjr): check if they have the same handle
                mem::forget(r);
            }
            entry::Entry::Vacant(e) => {
                e.insert(Entry::new(r, 0));
            }
        }
    }

    pub(crate) fn open_or_create_resource(&self, h: K, r: R) {
        match self.table.entry(h) {
            entry::Entry::Occupied(mut e) => {
                // TODO(cjr): check if they have the same handle
                e.get_mut().open();
                mem::forget(r);
            }
            entry::Entry::Vacant(e) => {
                e.insert(Entry::new(r, 1));
            }
        }
    }

    pub(crate) fn open_resource(&self, h: &K) -> Result<(), Error> {
        // increase the refcnt
        self.table.get(h).map(|r| r.open()).ok_or(Error::NotFound)
    }

    /// Reduces the reference counter by one. Returns the resource when it is the last instance
    /// in the table.
    pub(crate) fn close_resource(&self, h: &K) -> Result<Option<Arc<R>>, Error> {
        let close = self
            .table
            .get(h)
            .map(|r| r.close())
            .ok_or(Error::NotFound)?;
        if close {
            let r = self.table.remove(h).unwrap();
            return Ok(Some(r.1.data()));
        }
        Ok(None)
    }
}
