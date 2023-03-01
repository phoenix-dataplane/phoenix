//! Control path commands.
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use phoenix_api::addrinfo;
use phoenix_api::net;
use phoenix_api::net::returned;
use phoenix_api::net::{ConnParam, QpInitAttr};
use phoenix_api::Handle;

type IResult<T> = Result<T, phoenix_api::Error>;

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
        Option<net::ProtectionDomain>,
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
    CmCreateQp(Handle, Option<net::ProtectionDomain>, QpInitAttr),

    Disconnect(net::CmId),
    DestroyId(net::CmId),

    // reference counting
    OpenPd(net::ProtectionDomain),
    OpenCq(net::CompletionQueue),
    OpenQp(net::QueuePair),

    // ibverbs
    RegMr(net::ProtectionDomain, usize, net::AccessFlags),
    CreateCq(net::VerbsContext, i32, u64),

    DeallocPd(net::ProtectionDomain),
    DestroyCq(net::CompletionQueue),
    DestroyQp(net::QueuePair),

    DeregMr(net::MemoryRegion),

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
    OpenCq(u32),
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
