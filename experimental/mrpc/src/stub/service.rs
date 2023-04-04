use std::sync::Arc;

use phoenix_api::rpc::{MessageErased, MessageMeta, RpcMsgType};

use super::RpcData;
use crate::{RRef, ReadHeap, WRef, WRefOpaque};

/// A trait to provide a static reference to the service's name and ID.
/// This is used for routing service's within the router.
pub trait NamedService {
    /// The `Service-ID` corresponds to a [CRC32] hash of [`Self::NAME`].
    ///
    /// [CRC32]: https://docs.rs/crc32fast/latest/crc32fast/fn.hash.html
    const SERVICE_ID: u32;
    /// The `Service-Name` as described [here].
    ///
    /// [here]: https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-HTTP2.md#requests
    const NAME: &'static str = "";
}

/// A trait implemented by generated code.
#[crate::async_trait]
pub trait Service {
    /// Resolves to a type-erased [`WRef`] and the [type-erased RPC descriptor][MessageErased] for the reply.
    async fn call(
        &self,
        req: MessageErased,
        read_heap: Arc<ReadHeap>,
    ) -> (WRefOpaque, MessageErased);
}

#[doc(hidden)]
pub fn service_pre_handler<T: Unpin>(req: &MessageErased, read_heap: Arc<ReadHeap>) -> RRef<T> {
    RRef::new(req, read_heap)
}

#[doc(hidden)]
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
