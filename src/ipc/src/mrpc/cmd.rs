//! mRPC control path commands.
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use super::control_plane::TransportType;

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    SetTransport(TransportType),
    Connect(SocketAddr),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CompletionKind {
    SetTransport,
    Connect,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion(pub IResult<CompletionKind>);
