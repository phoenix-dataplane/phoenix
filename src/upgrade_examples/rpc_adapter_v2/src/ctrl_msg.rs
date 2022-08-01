use serde::{Deserialize, Serialize};

// Control message between two RpcAdapterEngine instances.
// i.e., two endpoints of a connection.
// NOTE(wyj): for now we simply use it to exchange versions
// we may use it for other purposes in the future.
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ControlMessage {
    ExchangeVersion(u64),
}

mod sa {
    use super::ControlMessage;
    use interface::rpc::MessageMeta;
    use static_assertions::const_assert;
    use std::mem::size_of;

    const_assert!(size_of::<ControlMessage>() < size_of::<MessageMeta>());
}
