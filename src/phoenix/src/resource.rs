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
    #[error("Slab is full or the maximum number of shards has been reached, please adjust the slab's configuration")]
    SlabFull,
}

#[derive(Debug)]
pub struct ResourceTableGeneric<K: Eq + std::hash::Hash, R> {
    table: DashMap<K, Entry<R>, FnvBuildHasher>,
}

pub type ResourceTable<R> = ResourceTableGeneric<Handle, R>;

impl<K: Eq + std::hash::Hash, R> Default for ResourceTableGeneric<K, R> {
    fn default() -> Self {
        ResourceTableGeneric {
            table: DashMap::default(),
        }
    }
}

// TODO(cjr): maybe remove this Entry, since DashMap already returns a Ref/RefMut.
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
    pub fn inner(&self) -> &DashMap<K, Entry<R>, FnvBuildHasher> {
        &self.table
    }

    pub fn insert(&self, h: K, r: R) -> Result<(), Error> {
        match self.table.insert(h, Entry::new(r, 1)) {
            Some(_) => Err(Error::Exists),
            None => Ok(()),
        }
    }

    pub fn get(&self, h: &K) -> Result<Arc<R>, Error> {
        self.table.get(h).map(|r| r.data()).ok_or(Error::NotFound)
    }

    pub fn get_dp(&self, h: &K) -> Result<Arc<R>, Error> {
        self.table.get(h).map(|r| r.data()).ok_or(Error::NotFound)
    }

    /// Occupy en entry by only inserting a value but not incrementing the reference count.
    pub fn occupy_or_create_resource(&self, h: K, r: R) {
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

    pub fn open_or_create_resource(&self, h: K, r: R) {
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

    pub fn open_resource(&self, h: &K) -> Result<(), Error> {
        // increase the refcnt
        self.table.get(h).map(|r| r.open()).ok_or(Error::NotFound)
    }

    /// Reduces the reference counter by one. Returns the resource when it is the last instance
    /// in the table.
    pub fn close_resource(&self, h: &K) -> Result<Option<Arc<R>>, Error> {
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

use crate::page_padded::PagePadded;
use sharded_slab::Slab;

#[derive(Debug)]
pub struct ResourceSlab<R> {
    // fast path slab
    slab: Arc<Slab<PagePadded<R>>>,
    // slow path table, Handle -> key in the slab
    table: ResourceTable<usize>,
    // key -> Handle, this is like a weak reference
    inverse_table: DashMap<usize, Handle>,
}

impl<R> Default for ResourceSlab<R> {
    fn default() -> Self {
        ResourceSlab {
            slab: Arc::new(Slab::new()),
            table: ResourceTable::default(),
            inverse_table: DashMap::default(),
        }
    }
}

impl<R> Drop for ResourceSlab<R> {
    fn drop(&mut self) {
        for entry in self.inverse_table.iter() {
            let k = entry.key();
            // println!("dropping k: {}", k);
            let _ = self.slab.remove(*k);
        }
    }
}

impl<R> ResourceSlab<R> {
    pub fn insert(&self, r: R) -> Result<usize, Error> {
        unsafe { libnuma_sys::numa_set_localalloc() };
        match self.slab.insert(PagePadded::new(r)) {
            Some(key) => {
                let handle = Handle(key as u64);
                self.associate(handle, key)?;
                Ok(key)
            }
            None => Err(Error::SlabFull),
        }
    }

    #[inline]
    pub fn get(&self, key: usize) -> Result<sharded_slab::OwnedEntry<PagePadded<R>>, Error> {
        match Slab::get_owned(Arc::clone(&self.slab), key) {
            Some(r) => Ok(r),
            None => Err(Error::NotFound),
        }
    }

    #[inline]
    pub fn get_dp(&self, key: usize) -> Result<sharded_slab::Entry<'_, PagePadded<R>>, Error> {
        match self.slab.get(key) {
            Some(r) => Ok(r),
            None => Err(Error::NotFound),
        }
    }

    #[inline]
    pub fn get_handle_from_key(&self, key: usize) -> Result<Handle, Error> {
        let h = self.inverse_table.get(&key).ok_or(Error::NotFound)?;
        Ok(*h)
    }

    pub fn open_resource_by_key(&self, key: usize) -> Result<(), Error> {
        let h = self.inverse_table.get(&key).ok_or(Error::NotFound)?;
        self.open_resource_by_handle(&h)
    }

    pub fn open_resource_by_handle(&self, h: &Handle) -> Result<(), Error> {
        self.table.open_resource(h)
    }

    pub fn close_resource_by_key(&self, key: usize) -> Result<Option<PagePadded<R>>, Error> {
        let href = self.inverse_table.get(&key).ok_or(Error::NotFound)?;
        let handle = *href;
        drop(href);
        self.close_resource_by_handle(&handle)
    }

    pub fn close_resource_by_handle(&self, h: &Handle) -> Result<Option<PagePadded<R>>, Error> {
        let close = self.table.close_resource(h)?;
        if let Some(key) = close {
            match self.slab.take(*key) {
                Some(r) => {
                    self.inverse_table.remove(&*key);
                    Ok(Some(r))
                }
                None => Err(Error::NotFound),
            }
        } else {
            Ok(None)
        }
    }

    #[inline]
    fn associate(&self, handle: Handle, key: usize) -> Result<(), Error> {
        // log::warn!("handle: {:?}, key: {}", handle, key);
        // log::warn!("self.inverse_table: {:?}", self.inverse_table);
        if self.inverse_table.insert(key, handle).is_some() {
            return Err(Error::Exists);
        }
        // log::warn!("self.table: {:?}", self.table);
        self.table.insert(handle, key)
    }

    /// Insert the value if the handle does not exist in the table. Otherwise, do nothing.
    /// Returns the key in the slab.
    pub fn occupy_or_create_resource(&self, h: Handle, r: R) -> Result<usize, Error> {
        if let Ok(key) = self.table.get(&h) {
            mem::forget(r);
            Ok(*key)
        } else {
            unsafe { libnuma_sys::numa_set_localalloc() };
            match self.slab.insert(PagePadded::new(r)) {
                Some(key) => {
                    self.associate(h, key)?;
                    Ok(key)
                }
                None => Err(Error::SlabFull),
            }
        }
    }
}
