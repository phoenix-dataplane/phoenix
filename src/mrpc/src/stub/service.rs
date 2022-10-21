use std::sync::Arc;

use interface::rpc::{MessageErased, MessageMeta, RpcMsgType};

use super::RpcData;
use crate::{RRef, ReadHeap, WRef, WRefOpaque};

// Service traits
pub trait NamedService {
    const SERVICE_ID: u32;
    const NAME: &'static str = "";
}

#[crate::async_trait]
pub trait Service {
    // Resolves to a type-erased WRef and the type-erased RPC descriptor for the reply.
    async fn call(
        &self,
        req: MessageErased,
        read_heap: Arc<ReadHeap>,
    ) -> (WRefOpaque, MessageErased);
}

pub fn service_pre_handler<T: Unpin>(req: &MessageErased, read_heap: Arc<ReadHeap>) -> RRef<T> {
    RRef::new(req, read_heap)
}

pub fn service_post_handler<T: RpcData>(
    reply: WRef<T>,
    req_opaque: &MessageErased,
) -> (WRefOpaque, MessageErased) {
    // construct meta
    let meta = MessageMeta {
        msg_type: RpcMsgType::Response,
        ..req_opaque.meta
    };

    let reply_opaque = WRef::clone(&reply).into_opaque();

    let (ptr_app, ptr_backend) = reply.into_shmptr().to_raw_parts();
    let erased = MessageErased {
        meta,
        shm_addr_app: ptr_app.addr().get(),
        shm_addr_backend: ptr_backend.addr().get(),
    };

    (reply_opaque, erased)
}
