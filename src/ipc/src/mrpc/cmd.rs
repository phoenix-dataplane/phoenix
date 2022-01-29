//! mRPC control path commands.
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    Connect(SocketAddr),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CompletionKind {
    Connect,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion(pub IResult<CompletionKind>);
