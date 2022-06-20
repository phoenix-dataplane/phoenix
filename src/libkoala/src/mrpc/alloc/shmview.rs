use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ops::Deref;

use arrayvec::ArrayVec;

use ipc::mrpc::dp::{WorkRequest, WrIdentifier, RECV_RECLAIM_BS};

use super::boxed::Box;
use crate::mrpc::MRPC_CTX;

pub(crate) mod from_backend {
    pub trait CloneFromBackendOwned {
        type BackendOwned;

        fn clone_from_backend_owned(_: &Self::BackendOwned) -> Self;
    }
}

use from_backend::CloneFromBackendOwned;

pub struct ShmView<'a, A: CloneFromBackendOwned> {
    inner: ManuallyDrop<
        Box<<A as CloneFromBackendOwned>::BackendOwned, crate::salloc::owner::BackendOwned>,
    >,
    wr_id: WrIdentifier,
    reclamation_buffer: &'a RefCell<ArrayVec<u32, RECV_RECLAIM_BS>>,
}

impl<'a, A: CloneFromBackendOwned> ShmView<'a, A> {
    pub(crate) fn new_from_backend_owned(
        backend_owned: Box<
            <A as CloneFromBackendOwned>::BackendOwned,
            crate::salloc::owner::BackendOwned,
        >,
        wr_id: WrIdentifier,
        reclamation_buffer: &'a RefCell<ArrayVec<u32, RECV_RECLAIM_BS>>,
    ) -> Self {
        ShmView {
            inner: ManuallyDrop::new(backend_owned),
            wr_id,
            reclamation_buffer,
        }
    }
}

impl<'a, A: CloneFromBackendOwned> Drop for ShmView<'a, A> {
    fn drop(&mut self) {
        let mut borrow = self.reclamation_buffer.borrow_mut();

        // only reclaim recv mr when the buffer is full
        // it is fine that there are some leftover messages in the buffer,
        // as all the recv mrs will be deallocated when the connection is closed
        borrow.push(self.wr_id.1);
        if borrow.is_full() {
            let msgs: [MaybeUninit<u32>; RECV_RECLAIM_BS] = MaybeUninit::uninit_array();
            let mut msgs = unsafe { MaybeUninit::array_assume_init(msgs) };
            msgs.copy_from_slice(borrow.as_slice());
            borrow.clear();

            let conn_id = self.wr_id.0;
            let reclaim_wr = WorkRequest::ReclaimRecvBuf(conn_id, msgs);
            MRPC_CTX.with(move |ctx| {
                let mut sent = false;
                while !sent {
                    let _ = ctx.service.enqueue_wr_with(|ptr, _count| unsafe {
                        ptr.cast::<WorkRequest>().write(reclaim_wr);
                        sent = true;
                        1
                    });
                }
            });
        }
    }
}

impl<'a, A: CloneFromBackendOwned> ShmView<'a, A> {
    pub fn into_owned(self) -> A {
        <A as CloneFromBackendOwned>::clone_from_backend_owned(&self.inner)
    }
}

impl<'a, A: CloneFromBackendOwned> Deref for ShmView<'a, A> {
    type Target = <A as CloneFromBackendOwned>::BackendOwned;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, A> Eq for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Eq,
{
}

impl<'a, A> Ord for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Ord,
{
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<'a, 'b, A, B> PartialEq<ShmView<'b, B>> for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned:
        PartialEq<<B as CloneFromBackendOwned>::BackendOwned>,
    B: CloneFromBackendOwned,
{
    #[inline]
    fn eq(&self, other: &ShmView<'b, B>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl<'a, A> PartialOrd for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: PartialOrd,
{
    #[inline]
    fn partial_cmp(&self, other: &ShmView<A>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<'a, A> fmt::Debug for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<'a, A> fmt::Display for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'a, A> Hash for ShmView<'a, A>
where
    A: CloneFromBackendOwned,
    <A as CloneFromBackendOwned>::BackendOwned: Hash,
{
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}

impl<'a, A: CloneFromBackendOwned> AsRef<<A as CloneFromBackendOwned>::BackendOwned>
    for ShmView<'a, A>
{
    fn as_ref(&self) -> &<A as CloneFromBackendOwned>::BackendOwned {
        &**self
    }
}
