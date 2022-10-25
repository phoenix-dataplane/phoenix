use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use uapi::Handle;

type IResult<T> = Result<T, uapi::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    ListConnection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub sock: Handle,
    pub local: SocketAddr,
    pub peer: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {
    ListConnection(Vec<Connection>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
