//! Socket Control path commands.
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use interface::Handle;

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    Bind(SocketAddr),
    Accept(Handle),
    Connect(SocketAddr),
    SetSockOption(Handle),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompletionKind {
    Bind(Handle),
    Accept(Handle),
    Connect(Handle),
    SetSockOption,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion(pub IResult<CompletionKind>);
