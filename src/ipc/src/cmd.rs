//! Control path commands.
use engine::SchedulingMode;
use serde::{Deserialize, Serialize};
use std::ops::Range;

use crate::interface::{QpInitAttrOwned, ConnParamOwned};
use interface::{
    Handle,
    addrinfo,
};

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
    CreateEp(
        addrinfo::AddrInfo,
        Option<Handle>,
        Option<QpInitAttrOwned>,
    ),
    Listen(Handle, i32),
    Accept(Handle, Option<ConnParamOwned>),
    GetRequest(Handle),
    Connect(Handle, Option<ConnParamOwned>),
    RegMsgs(Handle, Range<u64>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// name of the OneShotServer
    NewClient(SchedulingMode, String),
    HelloBack(i32),

    GetAddrInfo(IResult<addrinfo::AddrInfo>),
    // handle of cmid
    CreateEp(IResult<Handle>), // TODO(lsh): Handle to CmIdOwned
    Listen(IResult<()>),
    Accept(IResult<()>),
    GetRequest(IResult<Handle>),
    Connect(IResult<()>),
    RegMsgs(IResult<Handle>),
}
