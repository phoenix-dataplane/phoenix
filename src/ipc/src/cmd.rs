//! Control path commands.
use std::net::SocketAddr;

use engine::SchedulingMode;
use serde::{Deserialize, Serialize};

use interface::returned;
use interface::{addrinfo, ConnParam, Handle, QpInitAttr};

type IResult<T> = Result<T, interface::Error>;

#[derive(Serialize, Deserialize)]
pub enum Request {
    NewClient(SchedulingMode),
    Hello(i32),

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
    Accept(Handle, Option<ConnParam>),
    Connect(Handle, Option<ConnParam>),

    CreateId(Option<interface::EventChannel>, addrinfo::PortSpace),
    BindAddr(Handle, SocketAddr),
    ResolveAddr(Handle, SocketAddr),
    ResolveRoute(Handle, i32),
    CmCreateQp(
        Handle,
        Option<interface::ProtectionDomain>,
        Option<QpInitAttr>,
    ),

    Disconnect(interface::CmId),
    DestroyId(interface::CmId),

    // reference counting
    OpenPd(interface::ProtectionDomain),
    OpenCq(interface::CompletionQueue),
    OpenQp(interface::QueuePair),

    // ibverbs
    RegMr(interface::ProtectionDomain, usize, interface::AccessFlags),

    DeallocPd(interface::ProtectionDomain),
    DestroyCq(interface::CompletionQueue),
    DestroyQp(interface::QueuePair),

    DeregMr(interface::MemoryRegion),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResponseKind {
    /// .0: the requested scheduling mode
    /// .1: name of the OneShotServer
    /// .2: data path work queue capacity
    /// .3: data path completion queue capacity
    NewClient(SchedulingMode, String, usize, usize),
    HelloBack(i32),

    // rdmacm
    GetAddrInfo(addrinfo::AddrInfo),
    // handle of cmid, handle of inner qp
    CreateEp(returned::CmId),
    Listen,
    GetRequest(returned::CmId),
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
    OpenCq,
    OpenQp,

    // ibverbs
    RegMr(returned::MemoryRegion),

    DeallocPd,
    DestroyCq,
    DestroyQp,

    DeregMr,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
