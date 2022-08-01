//! Common date types for engine
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SchedulingMode {
    Dedicate,
    Spread,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineType {
    RdmaTransport,
    RdmaConnMgmt,
    TcpTransport,
    Mrpc,
    RpcAdapter,
    RpcAdapterAcceptor,
    Overload,
    RateLimit,
    Salloc,
}
