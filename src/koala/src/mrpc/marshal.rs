use serde::{Serialize, Deserialize};

use interface::Handle;
use interface::rpc::MessageMeta;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct ShmBuf {
    pub shm_id: Handle,
    pub offset: u32,
    pub len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SgList(pub Vec<ShmBuf>);

pub(crate) trait Marshal {
    type Error;
    fn marshal(&self) -> Result<SgList, Self::Error>;
}

pub(crate) trait Unmarshal: Sized {
    type Error;
    fn unmarshal(sg_list: SgList) -> Result<Self, Self::Error>;
}

impl Marshal for MessageMeta {
    type Error = ();
    fn marshal(&self) -> Result<SgList, Self::Error> {
        Ok(SgList(vec![]))
    }
}

impl Unmarshal for MessageMeta {
    type Error = ();
    fn unmarshal(mut sg_list: SgList) -> Result<Self, Self::Error> {
        if sg_list.0.len() != 1 {
            return Err(());
        }
        unimplemented!()
        // sg_list.0[0]
    }
}

pub(crate) trait RpcMessage: Send {
    fn conn_id(&self) -> u32;
    fn func_id(&self) -> u32;
    fn call_id(&self) -> u64; // unique id
    fn len(&self) -> u64;
    fn is_request(&self) -> bool;
    fn marshal(&self) -> SgList;
}
