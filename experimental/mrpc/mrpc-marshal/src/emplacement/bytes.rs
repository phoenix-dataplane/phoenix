use std::mem;

use crate::shadow::Vec;
use crate::{AddressArbiter, ExcavateContext, MarshalError, SgE, SgList, UnmarshalError};
use shm::ptr::ShmPtr;

#[inline]
pub fn emplace(val: &Vec<u8>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if val.is_empty() {
        return Ok(());
    }

    let buf_ptr = val.shm_non_null().as_ptr_backend().addr();
    let buf_len = val.len() * mem::size_of::<u8>();
    sgl.0.push(SgE {
        ptr: buf_ptr,
        len: buf_len,
    });

    Ok(())
}

#[inline]
pub fn emplace_optional(val: &Option<Vec<u8>>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if let Some(bytes) = val {
        emplace(bytes, sgl)?;
    }

    Ok(())
}

#[inline]
pub fn emplace_repeated(val: &Vec<Vec<u8>>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if val.is_empty() {
        return Ok(());
    }

    // emplace meta block for repeated
    let buf_ptr = val.shm_non_null().as_ptr_backend().addr();
    let buf_len = val.len() * mem::size_of::<Vec<u8>>();
    sgl.0.push(SgE {
        ptr: buf_ptr,
        len: buf_len,
    });

    // emplace the items
    for bytes in val {
        emplace(bytes, sgl)?;
    }

    Ok(())
}

#[inline]
pub unsafe fn excavate<'a, A: AddressArbiter>(
    val: &mut Vec<u8>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if val.is_empty() {
        // *val = Vec::new(); // WARNING(cjr): This drops the *val which is undesired;
        // unsafe { ptr::write(val, Vec::new()) };
        // This will generate the exact same code as ptr::write when opt-level >= 1, but does not
        // require unsafe.
        mem::forget(mem::replace(val, Vec::new()));
        return Ok(());
    }

    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
    let expected = val.len() * mem::size_of::<u8>();
    if buf_sge.len != expected {
        return Err(UnmarshalError::SgELengthMismatch {
            expected,
            actual: buf_sge.len,
        });
    }

    let backend_addr = buf_sge.ptr;
    let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;
    mem::forget(mem::replace(val, unsafe {
        Vec::from_raw_parts(
            app_addr as *mut u8,
            backend_addr as *mut u8,
            val.len(),
            val.len(),
        )
    }));

    Ok(())
}

#[inline]
pub unsafe fn excavate_optional<'a, A: AddressArbiter>(
    val: &mut Option<Vec<u8>>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if let Some(bytes) = val {
        excavate(bytes, ctx);
    }

    Ok(())
}

#[inline]
pub unsafe fn excavate_repeated<'a, A: AddressArbiter>(
    val: &mut Vec<Vec<u8>>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if val.is_empty() {
        mem::forget(mem::replace(val, Vec::new()));
        return Ok(());
    }

    // excavate meta
    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
    let expected = val.len() * mem::size_of::<Vec<u8>>();
    if buf_sge.len != expected {
        return Err(UnmarshalError::SgELengthMismatch {
            expected,
            actual: buf_sge.len,
        });
    }

    let backend_addr = buf_sge.ptr;
    let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;
    mem::forget(mem::replace(val, unsafe {
        Vec::from_raw_parts(
            app_addr as *mut Vec<u8>,
            backend_addr as *mut Vec<u8>,
            val.len(),
            val.len(),
        )
    }));

    for bytes in val.iter_mut() {
        excavate(bytes, ctx)?;
    }

    Ok(())
}

#[inline]
pub fn extent(val: &Vec<u8>) -> usize {
    // if !val.is_empty() {
    //     1
    // } else {
    //     0
    // }
    !val.is_empty() as usize
}

#[inline]
pub fn extent_optional(val: &Option<Vec<u8>>) -> usize {
    val.as_ref().map_or(0, extent)
}

#[inline]
pub fn extent_repeated(val: &Vec<Vec<u8>>) -> usize {
    if !val.is_empty() {
        1 + val.iter().map(extent).sum::<usize>()
    } else {
        0
    }
}
