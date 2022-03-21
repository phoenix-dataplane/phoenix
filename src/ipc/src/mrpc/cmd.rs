//! mRPC control path commands.
use std::{net::SocketAddr, os::unix::prelude::RawFd};

use serde::{Deserialize, Serialize};

use super::control_plane::TransportType;
use interface::{returned, Handle};

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    SetTransport(TransportType),
    AllocShm(usize),
    Connect(SocketAddr),
    Bind(SocketAddr),
    NewMappedAddrs(Vec<(Handle, u64)>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CompletionKind {
    SetTransport,
    AllocShmInternal(returned::MemoryRegion, RawFd),
    AllocShm(returned::MemoryRegion),
    // connection handle, receive mrs
    ConnectInternal(Handle, Vec<returned::MemoryRegion>, Vec<RawFd>),
    Connect((Handle, Vec<returned::MemoryRegion>)),
    Bind(Handle),
    // These are actually commands which go by a reverse direction.
    NewConnectionInternal(Handle, Vec<returned::MemoryRegion>, Vec<RawFd>),
    NewConnection((Handle, Vec<returned::MemoryRegion>)),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion(pub IResult<CompletionKind>);
