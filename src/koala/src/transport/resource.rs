use std::collections::HashMap;
use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};

use interface::Handle;

use crate::transport::{DatapathError, Error};

#[derive(Debug)]
pub(crate) struct ResourceTable<R> {
    table: HashMap<Handle, Entry<R>>,
}

impl<R> Default for ResourceTable<R> {
    fn default() -> Self {
        ResourceTable {
            table: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Entry<R> {
    refcnt: AtomicUsize,
    data: R,
}

impl<R> Entry<R> {
    fn new(data: R) -> Self {
        Entry {
            refcnt: AtomicUsize::new(1),
            data,
        }
    }

    #[inline]
    fn data(&self) -> &R {
        &self.data
    }

    /// `Open` means increment the reference count.
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
    pub(crate) fn insert(&mut self, h: Handle, r: R) -> Result<(), Error> {
        match self.table.insert(h, Entry::new(r)) {
            Some(_) => Err(Error::Exists),
            None => Ok(()),
        }
    }

    pub(crate) fn get(&self, h: &Handle) -> Result<&R, Error> {
        self.table.get(h).map(|r| r.data()).ok_or(Error::NotFound)
    }

    pub(crate) fn get_dp(&self, h: &Handle) -> Result<&R, DatapathError> {
        self.table
            .get(h)
            .map(|r| r.data())
            .ok_or(DatapathError::NotFound)
    }

    pub(crate) fn open_or_create_resource(&mut self, h: Handle, r: R) {
        // TODO(cjr): check if they have the same handle
        self.table
            .entry(h)
            .and_modify(|e| e.open())
            .or_insert(Entry::new(r));
    }

    pub(crate) fn open_resource(&mut self, h: &Handle) -> Result<(), Error> {
        // increase the refcnt
        self.table.get(h).map(|r| r.open()).ok_or(Error::NotFound)
    }

    pub(crate) fn close_resource(&mut self, h: &Handle) -> Result<(), Error> {
        // NOTE(cjr): This is not concurrent safe.
        let close = self
            .table
            .get(h)
            .map(|r| r.close())
            .ok_or(Error::NotFound)?;
        if close {
            let r = self.table.remove(h).unwrap();
            // just to make the drop explicit
            mem::drop(r);
        }
        Ok(())
    }
}
