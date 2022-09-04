use super::bytes;
use crate::shadow::String;
use crate::shadow::Vec;
use crate::{AddressArbiter, ExcavateContext, MarshalError, SgE, SgList, UnmarshalError};
use shm::ptr::ShmPtr;

#[inline]
pub fn emplace(val: &String, sgl: &mut SgList) -> Result<(), MarshalError> {
    bytes::emplace(&val.buf, sgl)
}

#[inline]
pub fn emplace_optional(val: &Option<String>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if let Some(s) = val {
        if s.buf.len == 0 {
            return Ok(());
        }
        let buf_ptr = s.buf.buf.ptr.as_ptr_backend().addr();
        let buf_len = s.buf.len * std::mem::size_of::<u8>();
        let buf_sge = SgE {
            ptr: buf_ptr,
            len: buf_len,
        };
        sgl.0.push(buf_sge);
    }

    Ok(())
}

#[inline]
pub fn emplace_repeated(val: &Vec<String>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if val.len == 0 {
        return Ok(());
    }

    let buf_ptr = val.buf.ptr.as_ptr_backend().addr();
    let buf_len = val.len * std::mem::size_of::<String>();
    let buf_sge = SgE {
        ptr: buf_ptr,
        len: buf_len,
    };
    sgl.0.push(buf_sge);

    for s in val.iter().filter(|s| s.buf.len > 0) {
        emplace(s, sgl)?;
    }

    Ok(())
}

#[inline]
pub unsafe fn excavate<'a, A: AddressArbiter>(
    val: &mut String,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    bytes::excavate(&mut val.buf, ctx)
}

#[inline]
pub unsafe fn excavate_optional<'a, A: AddressArbiter>(
    val: &mut Option<String>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if let Some(s) = val {
        if s.buf.len == 0 {
            s.buf.buf.ptr = ShmPtr::dangling();
            s.buf.buf.cap = 0;
            return Ok(());
        }
        let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
        let expected = s.buf.len * std::mem::size_of::<u8>();
        if buf_sge.len != expected {
            return Err(UnmarshalError::SgELengthMismatch {
                expected,
                actual: buf_sge.len,
            });
        }
        let backend_addr = buf_sge.ptr;
        let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;
        let buf_ptr = ShmPtr::new(app_addr as *mut u8, backend_addr as *mut u8).unwrap();
        s.buf.buf.ptr = buf_ptr;
        s.buf.buf.cap = s.buf.len;
    }

    Ok(())
}

#[inline]
pub unsafe fn excavate_repeated<'a, A: AddressArbiter>(
    val: &mut Vec<String>,
    ctx: &mut ExcavateContext<'a, A>,
) -> Result<(), UnmarshalError> {
    if val.len == 0 {
        val.buf.ptr = ShmPtr::dangling();
        val.buf.cap = 0;
        return Ok(());
    }

    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
    let expected = val.len * std::mem::size_of::<String>();
    if buf_sge.len != expected {
        return Err(UnmarshalError::SgELengthMismatch {
            expected,
            actual: buf_sge.len,
        });
    }
    let backend_addr = buf_sge.ptr;
    let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;
    let buf_ptr = ShmPtr::new(app_addr as *mut String, backend_addr as *mut String).unwrap();
    val.buf.ptr = buf_ptr;
    val.buf.cap = val.len;

    for s in val.iter_mut() {
        if s.buf.len == 0 {
            s.buf.buf.ptr = ShmPtr::dangling();
            s.buf.buf.cap = 0;
        } else {
            let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
            let expected = s.buf.len * std::mem::size_of::<u8>();
            if buf_sge.len != expected {
                return Err(UnmarshalError::SgELengthMismatch {
                    expected,
                    actual: buf_sge.len,
                });
            };
            let backend_addr = buf_sge.ptr;
            let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;
            let buf_ptr = ShmPtr::new(app_addr as *mut u8, backend_addr as *mut u8).unwrap();
            s.buf.buf.ptr = buf_ptr;
            s.buf.buf.cap = s.buf.len;
        }
    }

    Ok(())
}

#[inline]
pub fn extent(val: &String) -> usize {
    if val.buf.len > 0 {
        1
    } else {
        0
    }
}

#[inline]
pub fn extent_optional(val: &Option<String>) -> usize {
    if let Some(s) = val {
        if s.buf.len > 0 {
            1
        } else {
            0
        }
    } else {
        0
    }
}

#[inline]
pub fn extent_repeated(val: &Vec<String>) -> usize {
    if val.len > 0 {
        1 + val.iter().filter(|s| s.buf.len > 0).count()
    } else {
        0
    }
}
