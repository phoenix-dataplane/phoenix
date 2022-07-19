use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use interface::engine::{EngineType, SchedulingMode};

use crate::mrpc;
use crate::transport::{rdma, tcp};

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    NewClient(SchedulingMode, EngineType),
    RdmaTransport(rdma::control_plane::Request),
    TcpTransport(tcp::control_plane::Request),
    Mrpc(mrpc::control_plane::Request),
    Upgrade,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {
    /// path of the engine's domain socket
    NewClient(PathBuf),
    /// .0: the requested scheduling mode
    /// .1: name of the OneShotServer
    /// .2: data path work queue capacity in bytes
    ConnectEngine {
        mode: SchedulingMode,
        one_shot_name: String,
        wq_cap: usize,
        cq_cap: usize,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
