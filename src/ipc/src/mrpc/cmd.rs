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
    // send a list of addr on recv_mr, corresponding to ptr of some SgList elements
    // since backend has knowledge how many SgE there are on a recv_mr (currently only 1)
    // we just need the start addr of each SgE from client
    // backend will manage addr -> recv_mr lookup and manage ref counting 
    // (if multiple SgEs are put on a single recv_mr in the future) 
    RecycleRecvMr(Vec<usize>)
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
