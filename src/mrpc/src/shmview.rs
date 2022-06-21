use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ops::Deref;

use interface::rpc::MessageErased;
use ipc::mrpc::dp::{WorkRequest, WrIdentifier, RECV_RECLAIM_BS};

use crate::alloc::Box;
use crate::stub::ReclaimBuffer;
use crate::MRPC_CTX;

pub struct ShmView<'a, T> {
    inner: ManuallyDrop<Box<T>>,
    wr_id: WrIdentifier,
    reclamation_buffer: &'a ReclaimBuffer,
}

// TODO(cjr): double-check this: on dropping, the inner type should not call its deallocator
// but the shared memory should be properly recycled by the backend
impl<'a, T> Drop for ShmView<'a, T> {
    fn drop(&mut self) {
        let mut borrow = self.reclamation_buffer.0.borrow_mut();

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

impl<'a, T> ShmView<'a, T> {
    pub(crate) fn new(msg: &MessageErased, reclamation_buffer: &'a ReclaimBuffer) -> Self {
        let ptr_app = msg.shm_addr_app as *mut T;
        let ptr_backend = ptr_app.with_addr(msg.shm_addr_backend);
        // SAFETY: The box is created directly from a shared memory.
        // We must ensure its drop function is never called by wrapping it in ShmView.
        let backend_owned = unsafe { Box::from_raw(ptr_app, ptr_backend) };
        let wr_id = WrIdentifier(msg.meta.conn_id, msg.meta.call_id);
        ShmView {
            inner: ManuallyDrop::new(backend_owned),
            wr_id,
            reclamation_buffer,
        }
    }
}

impl<'a, T: Clone> ShmView<'a, T> {
    pub fn into_owned(self) -> T {
        // Clone to get an owned data structure
        // ManuallyDrop::clone(&self.inner).clone()
        unimplemented!();
        // self.inner.clone()
    }
}

impl<'a, T> Deref for ShmView<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, T: Eq> Eq for ShmView<'a, T> {}

impl<'a, T: Ord> Ord for ShmView<'a, T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<'a, 'b, A: PartialEq<B>, B> PartialEq<ShmView<'b, B>> for ShmView<'a, A> {
    #[inline]
    fn eq(&self, other: &ShmView<'b, B>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl<'a, T: PartialOrd> PartialOrd for ShmView<'a, T> {
    #[inline]
    fn partial_cmp(&self, other: &ShmView<T>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<'a, T: fmt::Debug> fmt::Debug for ShmView<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<'a, T: fmt::Display> fmt::Display for ShmView<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'a, T: Hash> Hash for ShmView<'a, T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}

impl<'a, T> AsRef<T> for ShmView<'a, T> {
    fn as_ref(&self) -> &T {
        &**self
    }
}
