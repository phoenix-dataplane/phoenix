//! Control path commands.
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use interface::returned;
use interface::{addrinfo, ConnParam, Handle, QpInitAttr};

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    // rdmacm
    GetAddrInfo(
        Option<String>,
        Option<String>,
        Option<addrinfo::AddrInfoHints>,
    ),
    CreateEp(
        addrinfo::AddrInfo,
        Option<interface::ProtectionDomain>,
        Option<QpInitAttr>,
    ),
    Listen(Handle, i32),
    GetRequest(Handle),
    TryGetRequest(Handle),
    Accept(Handle, Option<ConnParam>),
    Connect(Handle, Option<ConnParam>),

    CreateId(addrinfo::PortSpace),
    BindAddr(Handle, SocketAddr),
    ResolveAddr(Handle, SocketAddr),
    ResolveRoute(Handle, i32),
    CmCreateQp(Handle, Option<interface::ProtectionDomain>, QpInitAttr),

    Disconnect(interface::CmId),
    DestroyId(interface::CmId),

    // reference counting
    OpenPd(interface::ProtectionDomain),
    OpenCq(interface::CompletionQueue),
    OpenQp(interface::QueuePair),

    // ibverbs
    RegMr(interface::ProtectionDomain, usize, interface::AccessFlags),
    CreateCq(interface::VerbsContext, i32, u64),

    DeallocPd(interface::ProtectionDomain),
    DestroyCq(interface::CompletionQueue),
    DestroyQp(interface::QueuePair),

    DeregMr(interface::MemoryRegion),

    // Other
    GetDefaultPds,
    GetDefaultContexts,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CompletionKind {
    // rdmacm
    GetAddrInfo(addrinfo::AddrInfo),
    // handle of cmid, handle of inner qp
    CreateEp(returned::CmId),
    Listen,
    GetRequest(returned::CmId),
    TryGetRequest(Option<returned::CmId>),
    Accept,
    Connect,

    CreateId(returned::CmId),
    BindAddr,
    ResolveAddr,
    ResolveRoute,
    CmCreateQp(returned::QueuePair),

    Disconnect,
    DestroyId,

    // reference counting
    OpenPd,
    // return the CQ's capacity
    OpenCq(usize),
    OpenQp,

    // ibverbs
    RegMr(returned::MemoryRegion),
    CreateCq(returned::CompletionQueue),

    DeallocPd,
    DestroyCq,
    DestroyQp,

    DeregMr,

    // Other
    GetDefaultPds(Vec<returned::ProtectionDomain>),
    GetDefaultContexts(Vec<returned::VerbsContext>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion(pub IResult<CompletionKind>);
