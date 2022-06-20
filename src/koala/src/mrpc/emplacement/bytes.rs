use crate::mrpc::marshal::{ExcavateContext, MarshalError, SgE, SgList, UnmarshalError};
use crate::mrpc::shadow::vec::Vec;
use ipc::shmalloc::ShmPtr;

#[inline(always)]
pub fn emplace(val: &Vec<u8>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if val.len == 0 {
        return Ok(());
    }
    let buf_ptr = val.buf.ptr.as_ptr_backend().addr();
    let buf_len = val.len * std::mem::size_of::<u8>();
    let buf_sge = SgE {
        ptr: buf_ptr,
        len: buf_len,
    };
    sgl.0.push(buf_sge);

    Ok(())
}

#[inline(always)]
pub fn emplace_optional(val: &Option<Vec<u8>>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if let Some(bytes) = val {
        if bytes.len == 0 {
            return Ok(());
        }
        let buf_ptr = bytes.buf.ptr.as_ptr_backend().addr();
        let buf_len = bytes.len * std::mem::size_of::<u8>();
        let buf_sge = SgE {
            ptr: buf_ptr,
            len: buf_len,
        };
        sgl.0.push(buf_sge);
    }

    Ok(())
}

#[inline(always)]
pub fn emplace_repeated(val: &Vec<Vec<u8>>, sgl: &mut SgList) -> Result<(), MarshalError> {
    if val.len == 0 {
        return Ok(());
    }
    let buf_ptr = val.buf.ptr.as_ptr_backend().addr();
    let buf_len = val.len * std::mem::size_of::<Vec<u8>>();
    let buf_sge = SgE {
        ptr: buf_ptr,
        len: buf_len,
    };
    sgl.0.push(buf_sge);
    for bytes in val.iter().filter(|bytes| bytes.len > 0) {
        let buf_ptr = bytes.buf.ptr.as_ptr_backend().addr();
        let buf_len = bytes.len * std::mem::size_of::<u8>();
        let buf_sge = SgE {
            ptr: buf_ptr,
            len: buf_len,
        };
        sgl.0.push(buf_sge);
    }

    Ok(())
}

#[inline(always)]
pub unsafe fn excavate<'a>(
    val: &mut Vec<u8>,
    ctx: &mut ExcavateContext<'a>,
) -> Result<(), UnmarshalError> {
    if val.len == 0 {
        val.buf.ptr = ShmPtr::dangling();
        val.buf.cap = 0;
        return Ok(());
    }
    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
    let expected = val.len * std::mem::size_of::<u8>();
    if buf_sge.len != expected {
        return Err(UnmarshalError::SgELengthMismatch {
            expected,
            actual: buf_sge.len,
        });
    }
    let backend_addr = buf_sge.ptr;
    let app_addr = ctx.salloc.resource.query_app_addr(backend_addr)?;
    let buf_ptr = ShmPtr::new(app_addr as *mut u8, backend_addr as *mut u8).unwrap();
    val.buf.ptr = buf_ptr;
    val.buf.cap = val.len;

    Ok(())
}

#[inline(always)]
pub unsafe fn excavate_optional<'a>(
    val: &mut Option<Vec<u8>>,
    ctx: &mut ExcavateContext<'a>,
) -> Result<(), UnmarshalError> {
    if let Some(bytes) = val {
        if bytes.len == 0 {
            bytes.buf.ptr = ShmPtr::dangling();
            bytes.buf.cap = 0;
            return Ok(());
        }
        let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
        let expected = bytes.len * std::mem::size_of::<u8>();
        if buf_sge.len != expected {
            return Err(UnmarshalError::SgELengthMismatch {
                expected,
                actual: buf_sge.len,
            });
        }
        let backend_addr = buf_sge.ptr;
        let app_addr = ctx.salloc.resource.query_app_addr(backend_addr)?;
        let buf_ptr = ShmPtr::new(app_addr as *mut u8, backend_addr as *mut u8).unwrap();
        bytes.buf.ptr = buf_ptr;
        bytes.buf.cap = bytes.len;
    }

    Ok(())
}

#[inline(always)]
pub unsafe fn excavate_repeated<'a>(
    val: &mut Vec<Vec<u8>>,
    ctx: &mut ExcavateContext<'a>,
) -> Result<(), UnmarshalError> {
    if val.len == 0 {
        val.buf.ptr = ShmPtr::dangling();
        val.buf.cap = 0;
        return Ok(());
    }

    let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
    let expected = val.len * std::mem::size_of::<Vec<u8>>();
    if buf_sge.len != expected {
        return Err(UnmarshalError::SgELengthMismatch {
            expected,
            actual: buf_sge.len,
        });
    }
    let backend_addr = buf_sge.ptr;
    let app_addr = ctx.salloc.resource.query_app_addr(backend_addr)?;
    let buf_ptr = ShmPtr::new(app_addr as *mut Vec<u8>, backend_addr as *mut Vec<u8>).unwrap();
    val.buf.ptr = buf_ptr;
    val.buf.cap = val.len;

    for bytes in val.iter_mut() {
        if bytes.len == 0 {
            bytes.buf.ptr = ShmPtr::dangling();
            bytes.buf.cap = 0;
        } else {
            let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
            let expected = bytes.len * std::mem::size_of::<u8>();
            if buf_sge.len != expected {
                return Err(UnmarshalError::SgELengthMismatch {
                    expected,
                    actual: buf_sge.len,
                });
            };
            let backend_addr = buf_sge.ptr;
            let app_addr = ctx.salloc.resource.query_app_addr(backend_addr)?;
            let buf_ptr = ShmPtr::new(app_addr as *mut u8, backend_addr as *mut u8).unwrap();
            bytes.buf.ptr = buf_ptr;
            bytes.buf.cap = bytes.len;
        }
    }

    Ok(())
}

#[inline(always)]
pub fn extent(val: &Vec<u8>) -> usize {
    if val.len > 0 {
        1
    } else {
        0
    }
}

#[inline(always)]
pub fn extent_optional(val: &Option<Vec<u8>>) -> usize {
    if let Some(bytes) = val {
        if bytes.len > 0 {
            1
        } else {
            0
        }
    } else {
        0
    }
}

#[inline(always)]
pub fn extent_repeated(val: &Vec<Vec<u8>>) -> usize {
    if val.len > 0 {
        1 + val.iter().filter(|bytes| bytes.len > 0).count()
    } else {
        0
    }
}
