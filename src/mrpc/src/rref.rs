use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::sync::Arc;

use interface::rpc::{CallId, MessageErased, RpcId, Token};
use ipc::mrpc::dp::{WorkRequest, RECV_RECLAIM_BS};
use shm::ptr::ShmPtr;

use crate::ReadHeap;
use crate::MRPC_CTX;

pub type ShmView<T> = RRef<T>;

#[derive(Debug)]
struct RRefInner<T> {
    rpc_id: RpcId,
    /// User associated context.
    token: Token,
    read_heap: Arc<ReadHeap>,
    data: ShmPtr<T>,
}

/// A thread-safe reference-counting pointer to the read-only shared memory heap.
pub struct RRef<T>(Arc<RRefInner<T>>);

// TODO(cjr): double-check this: on dropping, the inner type should not call its deallocator
// but the shared memory should be properly recycled by the backend
impl<T> Drop for RRefInner<T> {
    fn drop(&mut self) {
        let msgs: [MaybeUninit<CallId>; RECV_RECLAIM_BS] = MaybeUninit::uninit_array();
        let mut msgs = unsafe { MaybeUninit::array_assume_init(msgs) };
        msgs[0] = self.rpc_id.1;

        let conn_id = self.rpc_id.0;
        let reclaim_wr = WorkRequest::ReclaimRecvBuf(conn_id, msgs);
        MRPC_CTX.with(move |ctx| {
            let mut sent = false;
            while !sent {
                ctx.service
                    .enqueue_wr_with(|ptr, _count| unsafe {
                        ptr.cast::<WorkRequest>().write(reclaim_wr);
                        sent = true;
                        1
                    })
                    .expect("channel to backend corrupted");
            }
        });

        self.read_heap.decrement_refcnt();
    }
}

impl<T> RRef<T> {
    /// Constructs an `RRef<T>` from the given type-erased message on the read-only
    /// shared memory heap.
    #[must_use]
    #[inline]
    pub fn new(msg: &MessageErased, read_heap: Arc<ReadHeap>) -> Self {
        let ptr_app = msg.shm_addr_app as *mut T;
        let ptr_backend = ptr_app.with_addr(msg.shm_addr_backend);
        let backend_owned = ShmPtr::new(ptr_app, ptr_backend).unwrap();

        let rpc_id = RpcId::new(msg.meta.conn_id, msg.meta.call_id);

        // increase refcnt on the heap
        read_heap.increment_refcnt();

        RRef(Arc::new(RRefInner {
            rpc_id,
            token: Token(msg.meta.token as usize),
            read_heap,
            data: backend_owned,
        }))
    }

    /// Returns the user associated token.
    #[must_use]
    #[inline]
    pub fn token(&self) -> Token {
        self.0.token
    }
}

impl<T> Clone for RRef<T> {
    fn clone(&self) -> Self {
        RRef(Arc::clone(&self.0))
    }
}

impl<T> Deref for RRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

// TODO(cjr): This lifetime looks problematic
impl<T> AsRef<T> for RRef<T> {
    fn as_ref(&self) -> &T {
        // SAFETY: data is constructed from MessageErased on read heap. This expression is safe
        // only when the MessageErased is valid.
        unsafe { self.0.data.as_ref_app() }
    }
}

impl<T: Clone> RRef<T> {
    pub fn into_owned(self) -> T {
        // Clone to get an owned data structure
        // ManuallyDrop::clone(&self.inner).clone()
        todo!("implement a way to materialize an RRef");
        // self.inner.clone()
    }
}

impl<T: Eq> Eq for RRef<T> {}

impl<T: Ord> Ord for RRef<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<A: PartialEq<B>, B> PartialEq<RRef<B>> for RRef<A> {
    #[inline]
    fn eq(&self, other: &RRef<B>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl<T: PartialOrd> PartialOrd for RRef<T> {
    #[inline]
    fn partial_cmp(&self, other: &RRef<T>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<T: fmt::Debug> fmt::Debug for RRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: fmt::Display> fmt::Display for RRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: Hash> Hash for RRef<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}
