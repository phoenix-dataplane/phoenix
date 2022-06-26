macro_rules! numeric {
    ($ty:ty, $proto_ty:ident) => {
        pub mod $proto_ty {
            use crate::{ExcavateContext, MarshalError, SgE, SgList, UnmarshalError, AddressArbiter};
            use crate::shadow::vec::Vec;
            use ipc::ptr::ShmPtr;

            #[inline(always)]
            pub fn emplace_repeated(val: &Vec<$ty>, sgl: &mut SgList) -> Result<(), MarshalError> {
                if val.len == 0 {
                    return Ok(());
                }
                let ptr = val.buf.ptr.as_ptr_backend().addr();
                let len = val.len * std::mem::size_of::<$ty>();
                let sge = SgE { ptr, len };
                sgl.0.push(sge);

                Ok(())
            }

            #[inline(always)]
            pub fn excavate_repeated<'a, A: AddressArbiter>(
                val: &mut Vec<$ty>,
                ctx: &mut ExcavateContext<'a, A>,
            ) -> Result<(), UnmarshalError> {
                if val.len == 0 {
                    val.buf.ptr = ShmPtr::dangling();
                    val.buf.cap = 0;
                    return Ok(());
                }
                let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
                let expected = val.len * std::mem::size_of::<$ty>();
                if buf_sge.len != expected {
                    return Err(UnmarshalError::SgELengthMismatch {
                        expected,
                        actual: buf_sge.len,
                    });
                }
                let backend_addr = buf_sge.ptr;
                let app_addr = ctx.salloc.query_app_addr(backend_addr)?;
                let buf_ptr = ShmPtr::new(app_addr as *mut $ty, backend_addr as *mut $ty).unwrap();
                val.buf.ptr = buf_ptr;
                val.buf.cap = val.len;

                Ok(())
            }

            #[inline(always)]
            pub fn extent_repeated(val: &Vec<$ty>) -> usize {
                if val.len > 0 {
                    1
                } else {
                    0
                }
            }
        }
    };
}

numeric!(bool, bool);
numeric!(i32, int32);
numeric!(i64, int64);
numeric!(u32, uint32);
numeric!(u64, uint64);
numeric!(f32, float);
numeric!(f64, double);
