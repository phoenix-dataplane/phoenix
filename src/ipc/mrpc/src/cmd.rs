//! mRPC control path commands.
use std::{net::SocketAddr, os::unix::prelude::RawFd, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::control_plane::TransportType;
use interface::Handle;

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    SetTransport(TransportType),
    Connect(SocketAddr),
    Bind(SocketAddr),
    // The app notifies the backend with its mapped addresses
    // conn_handle, [mr_handle, addr]
    NewMappedAddrs(interface::Handle, Vec<(interface::Handle, usize)>),
    // Mapped address with mRPC engine's meta buffer ptrs to warmup the meta buffers.
    NewMappedAddrsInternal(
        interface::Handle,
        Vec<(interface::Handle, usize)>,
        Vec<usize>,
    ),
    UpdateProtos(Vec<String>),
    UpdateProtosInner(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadHeapRegion {
    pub handle: Handle,
    pub addr: usize,
    pub len: usize,
    pub file_off: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectResponse {
    pub conn_handle: Handle,
    pub read_regions: Vec<ReadHeapRegion>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CompletionKind {
    SetTransport,
    // connection handle, receive mrs
    ConnectInternal(ConnectResponse, Vec<RawFd>),
    Connect(ConnectResponse),
    Bind(Handle),
    // These are actually commands which go by a reverse direction.
    // conn_handle, (mr_handle, kaddr, len, file_off)
    // TODO(wyj): pass align
    NewConnectionInternal(ConnectResponse, Vec<RawFd>),
    NewConnection(ConnectResponse),
    // the acknowledgement
    NewMappedAddrs,
    UpdateProtos,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion(pub IResult<CompletionKind>);
