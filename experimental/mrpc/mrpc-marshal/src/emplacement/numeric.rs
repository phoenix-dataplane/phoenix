macro_rules! numeric {
    ($ty:ty, $proto_ty:ident) => {
        pub mod $proto_ty {
            use crate::shadow::Vec;
            use crate::{
                AddressArbiter, ExcavateContext, MarshalError, SgE, SgList, UnmarshalError,
            };
            use shm::ptr::ShmPtr;

            #[inline]
            pub fn emplace_repeated(val: &Vec<$ty>, sgl: &mut SgList) -> Result<(), MarshalError> {
                if val.is_empty() {
                    return Ok(());
                }

                let ptr = val.shm_non_null().as_ptr_backend().addr();
                let len = val.len() * std::mem::size_of::<$ty>();
                sgl.0.push(SgE { ptr, len });

                Ok(())
            }

            #[inline]
            pub fn excavate_repeated<'a, A: AddressArbiter>(
                val: &mut Vec<$ty>,
                ctx: &mut ExcavateContext<'a, A>,
            ) -> Result<(), UnmarshalError> {
                if val.is_empty() {
                    std::mem::forget(std::mem::replace(val, Vec::new()));
                    return Ok(());
                }

                let buf_sge = ctx.sgl.next().ok_or(UnmarshalError::SgListUnderflow)?;
                let expected = val.len() * std::mem::size_of::<$ty>();
                if buf_sge.len != expected {
                    return Err(UnmarshalError::SgELengthMismatch {
                        expected,
                        actual: buf_sge.len,
                    });
                }

                let backend_addr = buf_sge.ptr;
                let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;
                std::mem::forget(std::mem::replace(val, unsafe {
                    Vec::from_raw_parts(
                        app_addr as *mut $ty,
                        backend_addr as *mut $ty,
                        val.len(),
                        val.len(),
                    )
                }));

                Ok(())
            }

            #[inline]
            pub fn extent_repeated(val: &Vec<$ty>) -> usize {
                if !val.is_empty() {
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
