use crate::transport;

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Transport(transport::control_plane::Request),
    Mrpc(/*mrpc::Request*/),
}