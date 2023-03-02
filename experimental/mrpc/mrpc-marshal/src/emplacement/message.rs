use std::mem;
use std::ptr;

use crate::shadow::Vec;
use crate::{
    AddressArbiter, ExcavateContext, MarshalError, RpcMessage, SgE, SgList, UnmarshalError,
};

use shm::ptr::ShmPtr;

#[inline]
pub fn emplace<M: RpcMessage>(msg: &M, sgl: &mut SgList) -> Result<(), MarshalError> {
    msg.emplace(sgl)
}

#[inline]
pub fn emplace_optional<M: RpcMessage>(
    msg: &Option<M>,
    sgl: &mut SgList,
) -> Result<(), MarshalError> {
    if let Some(msg) = msg {
        msg.emplace(sgl)?;
    }
    Ok(())
}

#[inline]
pub fn emplace_repeated<M: RpcMessage>(
    msgs: &Vec<M>,
    sgl: &mut SgList,
) -> Result<(), MarshalError> {
    if msgs.is_empty() {
        return Ok(());
    }

    let buf_ptr = msgs.shm_non_null().as_ptr_backend().addr();
    let buf_len = msgs.len() * mem::size_of::<M>();
    sgl.0.push(SgE {
        ptr: buf_ptr,
        len: buf_len,
    });

    for msg in msgs.iter() {
        msg.emplace(sgl)?;
    }
    Ok(())
}

#[inline]
pub unsafe fn excavate<'a, M: RpcMessage, A: AddressArbiter>(
    msg: &mut M,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    msg.excavate(ctx)
}

#[inline]
pub unsafe fn excavate_optional<'a, M: RpcMessage, A: AddressArbiter>(
    msg: &mut Option<M>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if let Some(msg) = msg {
        msg.excavate(ctx)?;
    }

    Ok(())
}

#[inline]
pub unsafe fn excavate_repeated<'a, M: RpcMessage, A: AddressArbiter>(
    msgs: &mut Vec<M>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if msgs.is_empty() {
        mem::forget(mem::replace(msgs, Vec::new()));
        return Ok(());
    }

    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
    let expected = msgs.len() * mem::size_of::<M>();
    if buf_sge.len != expected {
        return Err(UnmarshalError::SgELengthMismatch {
            expected,
            actual: buf_sge.len,
        });
    }

    let backend_addr = buf_sge.ptr;
    let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;
    unsafe {
        ptr::write(
            msgs,
            Vec::from_raw_parts(
                app_addr as *mut M,
                backend_addr as *mut M,
                msgs.len(),
                msgs.len(),
            ),
        )
    };

    for msg in msgs.iter_mut() {
        msg.excavate(ctx)?;
    }

    Ok(())
}

#[inline]
pub fn extent<M: RpcMessage>(msg: &M) -> usize {
    msg.extent()
}

#[inline]
pub fn extent_optional<M: RpcMessage>(msg: &Option<M>) -> usize {
    if let Some(msg) = msg {
        msg.extent()
    } else {
        0
    }
}

#[inline]
pub fn extent_repeated<M: RpcMessage>(msgs: &Vec<M>) -> usize {
    if !msgs.is_empty() {
        1 + msgs.iter().map(extent).sum::<usize>()
    } else {
        0
    }
}
