use std::cell::RefCell;
use std::sync::Arc;

use uapi::Handle;

use super::pending::PendingWRef;
use crate::{Error, ReadHeap};

/// A mRPC connection.
#[derive(Debug)]
pub(crate) struct Connection {
    inner: RefCell<Inner>,
}

#[derive(Debug)]
pub(crate) enum Inner {
    Dead(DeadConnection),
    Alive(AliveConnection),
}

#[derive(Debug)]
pub(crate) struct AliveConnection {
    /// mRPC connection handle, aka conn_id
    pub(crate) handle: Handle,
    pub(crate) read_heap: Arc<ReadHeap>,
    pub(crate) pending: PendingWRef,
}

#[derive(Debug)]
pub(crate) struct DeadConnection {
    /// mRPC connection handle, aka conn_id
    handle: Handle,
}

impl PartialEq for Connection {
    fn eq(&self, other: &Self) -> bool {
        self.handle() == other.handle()
    }
}

impl Eq for Connection {}

impl std::hash::Hash for Connection {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.handle().hash(state)
    }
}

impl Connection {
    #[inline]
    pub(crate) fn new(handle: Handle, read_heap: ReadHeap) -> Self {
        Connection {
            inner: RefCell::new(Inner::Alive(AliveConnection::new(handle, read_heap))),
        }
    }

    pub(crate) fn close(&self) {
        let mut inner = self.inner.borrow_mut();
        match &mut *inner {
            Inner::Alive(alive) => *inner = Inner::Dead(alive.close()),
            Inner::Dead(_dead) => panic!("Double close on {:?}", self.handle()),
        }
    }

    #[inline]
    pub(crate) fn handle(&self) -> Handle {
        let inner = self.inner.borrow();
        match &*inner {
            Inner::Alive(alive) => alive.handle,
            Inner::Dead(dead) => dead.handle,
        }
    }

    #[inline]
    pub(crate) fn map_alive<T, F: FnOnce(&AliveConnection) -> T>(&self, f: F) -> Result<T, Error> {
        let inner = self.inner.borrow();
        match &*inner {
            Inner::Alive(alive) => Ok(f(alive)),
            Inner::Dead(_) => Err(Error::ConnectionClosed),
        }
    }
}

impl AliveConnection {
    #[inline]
    pub(crate) fn new(handle: Handle, read_heap: ReadHeap) -> Self {
        Self {
            handle,
            read_heap: Arc::new(read_heap),
            pending: PendingWRef::new(),
        }
    }

    pub(crate) fn close(&mut self) -> DeadConnection {
        DeadConnection {
            handle: self.handle,
        }
        // todo!("notify the backend to close the connection");
    }
}
