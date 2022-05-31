pub mod shmptr;

pub use shmptr::ShmPtr;

// TODO(wyj): implement SwitchAddressSpace for all types
// currently none of them is implemented
pub unsafe trait SwitchAddressSpace {
    // An unsafe trait is unsafe to implement but safe to use.
    // The user of this trait does not need to satisfy any special condition.
    fn switch_address_space(&mut self) {}
}

// TODO(wyj): remove std's Vec
unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for Vec<T> {
    fn switch_address_space(&mut self) {
    }
}

// TODO(wyj): remove std's Vec
unsafe impl<T> SwitchAddressSpace for Vec<T> {
    default fn switch_address_space(&mut self) {
        
    }
}

unsafe impl SwitchAddressSpace for u8 {}
unsafe impl SwitchAddressSpace for u16 {}
unsafe impl SwitchAddressSpace for u32 {}
unsafe impl SwitchAddressSpace for u64 {}
unsafe impl SwitchAddressSpace for u128 {}

unsafe impl SwitchAddressSpace for i8 { }
unsafe impl SwitchAddressSpace for i16 { }
unsafe impl SwitchAddressSpace for i32 { }
unsafe impl SwitchAddressSpace for i64 { }
unsafe impl SwitchAddressSpace for i128 { }
unsafe impl SwitchAddressSpace for isize { }

unsafe impl SwitchAddressSpace for std::num::NonZeroU8 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroU16 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroU32 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroU64 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroU128 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroUsize { }

unsafe impl SwitchAddressSpace for std::num::NonZeroI8 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroI16 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroI32 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroI64 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroI128 { }
unsafe impl SwitchAddressSpace for std::num::NonZeroIsize { }

unsafe impl SwitchAddressSpace for f32 { }
unsafe impl SwitchAddressSpace for f64 { }

unsafe impl SwitchAddressSpace for bool { }
unsafe impl SwitchAddressSpace for () { }

unsafe impl SwitchAddressSpace for char { }

unsafe impl SwitchAddressSpace for std::time::Duration { }

unsafe impl<T> SwitchAddressSpace for std::marker::PhantomData<T> {}

macro_rules! tuple_switch_addr_space {
    ( $($name:ident)+) => (
        unsafe impl<$($name: SwitchAddressSpace),*> SwitchAddressSpace for ($($name,)*) {
            #[allow(non_snake_case)]
            fn switch_address_space(&mut self) {
                let ($(ref mut $name,)*) = *self;
                $($name.switch_address_space();)*
            }
        }
    );
}

tuple_switch_addr_space!(A);
tuple_switch_addr_space!(A B);
tuple_switch_addr_space!(A B C);
tuple_switch_addr_space!(A B C D);
tuple_switch_addr_space!(A B C D E);
tuple_switch_addr_space!(A B C D E F);
tuple_switch_addr_space!(A B C D E F G);
tuple_switch_addr_space!(A B C D E F G H);
tuple_switch_addr_space!(A B C D E F G H I);
tuple_switch_addr_space!(A B C D E F G H I J);
