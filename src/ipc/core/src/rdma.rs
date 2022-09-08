use crate::buf::Range;
use interface::rpc::{ImmFlags, RpcId};
/// Structure transmitted between rpc engine, schduler engine and rdma API
#[derive(Debug, Clone)]
pub struct RawRdmaMsgTx {
    /// Raw MR pointer
    pub mr: u64,
    /// Absolute base address and length. No corresponding MR required
    pub range: Range,
    /// RPC identifier
    pub rpc_id: RpcId,
}

impl RawRdmaMsgTx {
    /// Return true if all bits in flags are set.
    #[inline(always)]
    pub fn has_all(&self, flags: ImmFlags) -> bool {
        self.rpc_id.flag_bits.has_all(flags)
    }

    /// Return true if any bit in flags is set.
    #[inline(always)]
    pub fn has_any(&self, flags: ImmFlags) -> bool {
        self.rpc_id.flag_bits.has_any(flags)
    }

    /// Helper function to set inside rpc_id.flag_bits
    #[inline(always)]
    pub fn set_flag(&mut self, flags: ImmFlags) {
        self.rpc_id.flag_bits.set(flags)
    }
}
