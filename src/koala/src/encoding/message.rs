use ipc::shmalloc::ShmPtr;
use crate::mrpc::shadow::vec::Vec;
use crate::mrpc::marshal::{RpcMessage, SgList, SgE, ExcavateContext, MarshalError, UnmarshalError};

#[inline(always)]
pub fn emplace<M: RpcMessage>(msg: &M, sgl: &mut SgList) -> Result<(), MarshalError>{
    msg.emplace(sgl)
}

#[inline(always)]
pub fn emplace_optional<M: RpcMessage>(msg: &Optional<M>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if let Some(msg) = msg {
        msg.emplace(sgl)?;
    }
    Ok(())
}

#[inline(always)]
pub fn emplace_repeated<M: RpcMessage>(msgs: &Vec<M>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if msgs.len == 0 {
        return Ok(());
    }
    let buf_ptr = msgs.buf.ptr.as_ptr_backend().addr();
    let buf_len = msgs.len * std::mem::size_of::<M>();
    let buf_sge = SgE {
        ptr: buf_ptr,
        len: buf_len
    };
    sgl.push(buf_sge);
    for msg in msgs.iter() {
        msg.emplace(sgl)?;
    }
    Ok(())
}

#[inline(always)]
pub fn excavate<'a, M: RpcMessage>(
    msg: &mut M,
    ctx: &mut ExcavateContext<'a>,
) -> Result<(), UnmarshalError> {
    msg.excavate(ctx)
}

#[inline(always)]
pub fn excavate_optional<'a, M: RpcMessage>(
    msg: &mut Option<M>,
    ctx: &mut ExcavateContext<'a>
) -> Result<(), UnmarshalError> {
    if let Some(msg) = msg {
        msg.excavate(ctx)?;
    }

    Ok(())
}

#[inline(always)]
pub fn excavate_repeated<'a, M: RpcMessage>(
    msgs: &mut Vec<M>,
    ctx: &mut ExcavateContext<'a>
) -> Result<(), UnmarshalError> {
    if msgs.len == 0 {
        msgs.buf.ptr = ShmPtr::dangling();
        msgs.buf.cap = 0;
        return Ok(())
    }

    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow);
    let expected = msgs.len * std::mem::size_of::<M>();
    if buf_sge.len != expected {
        return Err(UnmarshalError::SgELengthMismatch {
            expected,
            actual: buf_sge.len
        })
    }
    let backend_addr = buf_sge.ptr;
    let app_addr = ctx.salloc
        .resource
        .query_app_addr(backend_addr)?;
    let buf_ptr = ShmPtr::new(
        app_addr as *mut Vec<u8>,
        backend_addr as *mut Vec<u8>
    ).unwrap();
    msgs.buf.ptr = buf_ptr;
    msgs.buf.cap = msgs.len;

    for msg in msgs.iter_mut() {
        msg.excavate(ctx)?;
    }

    Ok(())
}

#[inline(always)]
pub fn extent<M: RpcMessage>(msg: &M) -> usize {
    msg.extent()
}

#[inline(always)]
pub fn extent_optional<M: RpcMessage>(msg: &Optional<M>) -> usize {
    if let Some(msg) = msg {
        msg.extent();
    }
    else { 0 }
}

#[inline(always)]
pub fn extent_repeated<M: RpcMessage>(msgs: &Vec<M>) -> usize {
    if msgs.len > 0 {
        1 + msgs.iter().map(|msg| msg.extent()).sum()
    }
    else { 0 }
}