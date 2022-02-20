use crate::transport::{rdma, tcp};
use crate::mrpc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    RdmaTransport(rdma::control_plane::Request),
    TcpTransport(tcp::control_plane::Request),
    Mrpc(mrpc::control_plane::Request),
}
