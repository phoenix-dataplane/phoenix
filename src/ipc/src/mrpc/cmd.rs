//! mRPC control path commands.
use std::{net::SocketAddr, os::unix::prelude::RawFd};

use serde::{Deserialize, Serialize};

use super::control_plane::TransportType;
use interface::Handle;

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    SetTransport(TransportType),
    Connect(SocketAddr),
    Bind(SocketAddr),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CompletionKind {
    SetTransport,
    // connection handle, receive mrs
    ConnectInternal(Handle, Vec<(Handle, usize, usize, i64)>, Vec<RawFd>),
    Connect((Handle, Vec<(Handle, usize, usize, i64)>)),
    Bind(Handle),
    // These are actually commands which go by a reverse direction.
    // conn_handle, (mr_handle, kaddr, len, file_off)
    // TODO(wyj): pass align
    NewConnectionInternal(Handle, Vec<(Handle, usize, usize, i64)>, Vec<RawFd>),
    NewConnection((Handle, Vec<(Handle, usize, usize, i64)>)),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion(pub IResult<CompletionKind>);