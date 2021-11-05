//! Control path commands.
use engine::SchedulingMode;
use serde::{Deserialize, Serialize};
use std::ops::Range;

use crate::interface::{ConnParamOwned, QpInitAttrOwned};
use interface::{addrinfo, Handle};

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
    CreateEp(addrinfo::AddrInfo, Option<Handle>, Option<QpInitAttrOwned>),
    Listen(Handle, i32),
    GetRequest(Handle),
    Accept(Handle, Option<ConnParamOwned>),
    Connect(Handle, Option<ConnParamOwned>),
    RegMsgs(Handle, Range<u64>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResponseKind {
    /// name of the OneShotServer
    NewClient(SchedulingMode, String),
    HelloBack(i32),

    GetAddrInfo(addrinfo::AddrInfo),
    // handle of cmid
    CreateEp(Handle), // TODO(lsh): Handle to CmIdOwned
    Listen,
    GetRequest(Handle),
    Accept,
    Connect,
    RegMsgs(Handle),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
