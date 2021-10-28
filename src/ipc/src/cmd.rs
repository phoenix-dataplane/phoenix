//! Control path commands.
use engine::SchedulingMode;
use serde::{Deserialize, Serialize};

use crate::interface::QpInitAttrOwned;
use interface::{
    Handle,
    addrinfo,
};

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
        Vec<addrinfo::AddrInfo>,
        Option<Handle>,
        Option<QpInitAttrOwned>,
    ),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// name of the OneShotServer
    NewClient(SchedulingMode, String),
    HelloBack(i32),

    GetAddrInfo(Result<Vec<addrinfo::AddrInfo>, interface::Error>),
    // handle of cmid
    CreateEp(Result<Handle, interface::Error>),
}
