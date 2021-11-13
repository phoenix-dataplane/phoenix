//! Control path commands.
use engine::SchedulingMode;
use serde::{Deserialize, Serialize};

use interface::returned;
use interface::{addrinfo, ConnParam, Handle, QpInitAttr};

use crate::buf::Buffer;

type IResult<T> = Result<T, interface::Error>;

#[derive(Serialize, Deserialize)]
pub enum Request {
    NewClient(SchedulingMode),
    Hello(i32),

    GetAddrInfo(
        Option<String>,
        Option<String>,
        Option<addrinfo::AddrInfoHints>,
    ),
    CreateEp(addrinfo::AddrInfo, Option<Handle>, Option<QpInitAttr>),
    Listen(Handle, i32),
    GetRequest(Handle),
    Accept(Handle, Option<ConnParam>),
    Connect(Handle, Option<ConnParam>),
    RegMsgs(Handle, Buffer),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResponseKind {
    /// .0: the requested scheduling mode
    /// .1: name of the OneShotServer
    /// .2: data path work queue capacity
    /// .3: data path completion queue capacity
    NewClient(SchedulingMode, String, usize, usize),
    HelloBack(i32),

    GetAddrInfo(addrinfo::AddrInfo),
    // handle of cmid, handle of inner qp
    CreateEp(returned::CmId), // TODO(lsh): Handle to CmIdOwned
    Listen,
    GetRequest(returned::CmId),
    Accept,
    Connect,
    RegMsgs(Handle),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
