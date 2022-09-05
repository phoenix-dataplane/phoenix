use std::mem;

use crate::shadow::String;
use crate::shadow::Vec;
use crate::{AddressArbiter, ExcavateContext, MarshalError, SgE, SgList, UnmarshalError};
use shm::ptr::ShmPtr;

#[inline]
pub fn emplace(val: &String, sgl: &mut SgList) -> Result<(), MarshalError> {
    if val.is_empty() {
        return Ok(());
    }

    let buf_ptr = val.shm_non_null().as_ptr_backend().addr();
    let buf_len = val.as_bytes().len() * mem::size_of::<u8>();
    sgl.0.push(SgE {
        ptr: buf_ptr,
        len: buf_len,
    });

    Ok(())
}

#[inline]
pub fn emplace_optional(val: &Option<String>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if let Some(s) = val {
        emplace(s, sgl)?;
    }

    Ok(())
}

#[inline]
pub fn emplace_repeated(val: &Vec<String>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if val.is_empty() {
        return Ok(());
    }

    let buf_ptr = val.shm_non_null().as_ptr_backend().addr();
    let buf_len = val.len() * mem::size_of::<String>();
    sgl.0.push(SgE {
        ptr: buf_ptr,
        len: buf_len,
    });

    for s in val {
        emplace(s, sgl)?;
    }

    Ok(())
}

#[inline]
pub unsafe fn excavate<'a, A: AddressArbiter>(
    val: &mut String,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if val.is_empty() {
        *val = String::new();
        return Ok(());
    }

    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
    let expected = val.as_bytes().len() * mem::size_of::<u8>();
    if buf_sge.len != expected {
        return Err(UnmarshalError::SgELengthMismatch {
            expected,
            actual: buf_sge.len,
        });
    }

    let backend_addr = buf_sge.ptr;
    let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;
    *val = unsafe {
        String::from_raw_parts(
            app_addr as *mut u8,
            backend_addr as *mut u8,
            val.len(),
            val.len(),
        )
    };

    Ok(())
}

#[inline]
pub unsafe fn excavate_optional<'a, A: AddressArbiter>(
    val: &mut Option<String>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if let Some(s) = val {
        excavate(s, ctx)?;
    }

    Ok(())
}

#[inline]
pub unsafe fn excavate_repeated<'a, A: AddressArbiter>(
    val: &mut Vec<String>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if val.is_empty() {
        mem::forget(mem::replace(val, Vec::new()));
        return Ok(());
    }

    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
    let expected = val.len() * std::mem::size_of::<String>();
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
            app_addr as *mut String,
            backend_addr as *mut String,
            val.len(),
            val.len(),
        )
    }));

    for s in val.iter_mut() {
        excavate(s, ctx)?;
    }

    Ok(())
}

#[inline]
pub fn extent(val: &String) -> usize {
    if !val.is_empty() {
        1
    } else {
        0
    }
}

#[inline]
pub fn extent_optional(val: &Option<String>) -> usize {
    val.as_ref().map_or(0, extent)
}

#[inline]
pub fn extent_repeated(val: &Vec<String>) -> usize {
    if val.len() > 0 {
        1 + val.iter().map(extent).sum::<usize>()
    } else {
        0
    }
}
