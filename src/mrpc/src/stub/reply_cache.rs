use interface::rpc::{CallId, MessageErased, TransportStatus};
use thiserror::Error;

use slab::Slab;

#[derive(Error, Debug)]
pub(crate) enum Error {
    #[error("CallId {0} not found")]
    NotFound(CallId),
}

#[derive(Debug)]
pub(crate) struct ReplyCacheT<T> {
    // Each RPC identified by a call_id resolves to a Result<MessageErased, TransportStatus>
    slab: Slab<Option<T>>,
}

impl<T> Default for ReplyCacheT<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ReplyCacheT<T> {
    pub(crate) fn new() -> Self {
        ReplyCacheT { slab: Slab::new() }
    }

    #[inline]
    pub(crate) fn initiate_call(&mut self) -> CallId {
        self.slab.insert(None).into()
    }

    #[inline]
    pub(crate) fn update(&mut self, call_id: CallId, val: T) -> Result<(), Error> {
        match self.slab.get_mut(call_id.0 as usize) {
            Some(entry) => {
                entry.replace(val);
                return Ok(());
            }
            None => return Err(Error::NotFound(call_id)),
        }
    }

    #[inline]
    pub(crate) fn get(&self, call_id: CallId) -> Result<&Option<T>, Error> {
        self.slab
            .get(call_id.0 as usize)
            .ok_or(Error::NotFound(call_id))
    }
}

pub(crate) type ReplyCache = ReplyCacheT<Result<MessageErased, TransportStatus>>;
