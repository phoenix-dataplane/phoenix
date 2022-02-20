use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use engine::SchedulingMode;

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    NewClient(SchedulingMode),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {
    /// path of the engine's domain socket
    NewClient(PathBuf),
    /// .0: the requested scheduling mode
    /// .1: name of the OneShotServer
    /// .2: data path work queue capacity in bytes
    /// .3: data path completion queue capacity in bytes
    ConnectEngine {
        mode: SchedulingMode,
        one_shot_name: String,
        wq_cap: usize,
        cq_cap: usize,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
