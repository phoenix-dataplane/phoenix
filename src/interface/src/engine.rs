//! Common date types for engine
use serde::{Serialize, Deserialize};

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
    Overload,
    Salloc,
}
