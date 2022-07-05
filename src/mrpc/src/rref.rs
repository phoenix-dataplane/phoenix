use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::sync::Arc;

use interface::rpc::{MessageErased, RpcId};
use ipc::mrpc::dp::{WorkRequest, RECV_RECLAIM_BS};
use ipc::ptr::ShmPtr;

use crate::salloc::ReadHeap;
use crate::MRPC_CTX;

pub type ShmView<'a, T> = RRef<'a, T>;

#[derive(Debug)]
struct RRefInner<'a, T> {
    rpc_id: RpcId,
    read_heap: &'a ReadHeap,
    data: ShmPtr<T>,
}

pub struct RRef<'a, T>(Arc<RRefInner<'a, T>>);

// TODO(cjr): double-check this: on dropping, the inner type should not call its deallocator
// but the shared memory should be properly recycled by the backend
// impl<'a, T> Drop for ShmView<'a, T> {
impl<'a, T> Drop for RRefInner<'a, T> {
    fn drop(&mut self) {
        let msgs: [MaybeUninit<u32>; RECV_RECLAIM_BS] = MaybeUninit::uninit_array();
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

impl<'a, T> RRef<'a, T> {
    pub fn new(msg: &MessageErased, read_heap: &'a ReadHeap) -> Self {
        let ptr_app = msg.shm_addr_app as *mut T;
        let ptr_backend = ptr_app.with_addr(msg.shm_addr_backend);
        let backend_owned = ShmPtr::new(ptr_app, ptr_backend).unwrap();

        let rpc_id = RpcId(msg.meta.conn_id, msg.meta.call_id);

        // increase refcnt on the heap
        read_heap.increment_refcnt();

        RRef(Arc::new(RRefInner {
            rpc_id,
            read_heap,
            data: backend_owned,
        }))
    }
}

impl<'a, T> Clone for RRef<'a, T> {
    fn clone(&self) -> Self {
        RRef(self.0.clone())
    }
}

impl<'a, T> Deref for RRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

// TODO(cjr): This lifetime looks problematic
impl<'a, T> AsRef<T> for RRef<'a, T> {
    fn as_ref(&self) -> &T {
        // SAFETY: data is constructed from MessageErased on read heap. This expression is safe
        // only when the MessageErased is valid.
        unsafe { self.0.data.as_ref_app() }
    }
}

impl<'a, T: Clone> RRef<'a, T> {
    pub fn into_owned(self) -> T {
        // Clone to get an owned data structure
        // ManuallyDrop::clone(&self.inner).clone()
        todo!("implement a way to materialize an RRef");
        // self.inner.clone()
    }
}

impl<'a, T: Eq> Eq for RRef<'a, T> {}

impl<'a, T: Ord> Ord for RRef<'a, T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<'a, 'b, A: PartialEq<B>, B> PartialEq<RRef<'b, B>> for RRef<'a, A> {
    #[inline]
    fn eq(&self, other: &RRef<'b, B>) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl<'a, T: PartialOrd> PartialOrd for RRef<'a, T> {
    #[inline]
    fn partial_cmp(&self, other: &RRef<T>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<'a, T: fmt::Debug> fmt::Debug for RRef<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<'a, T: fmt::Display> fmt::Display for RRef<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'a, T: Hash> Hash for RRef<'a, T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}
