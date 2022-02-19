use crate::transport;
use crate::mrpc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Transport(transport::control_plane::Request),
    Mrpc(mrpc::control_plane::Request),
}
