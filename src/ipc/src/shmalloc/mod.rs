pub mod shmptr;
pub mod shm_non_null;

pub use shmptr::ShmPtr;
pub use shm_non_null::ShmNonNull;

// TODO(wyj): implement SwitchAddressSpace for all types
// currently none of them is implemented
pub unsafe trait SwitchAddressSpace {
    // An unsafe trait is unsafe to implement but safe to use.
    // The user of this trait does not need to satisfy any special condition.
    fn switch_address_space(&mut self);
}


// TODO(cjr): double-check if the code below is correct.
unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for Vec<T> {
    fn switch_address_space(&mut self) {

    }
}

// TODO(cjr): double-check if the code below is correct.
unsafe impl<T> SwitchAddressSpace for Vec<T> {
    default fn switch_address_space(&mut self) {
        
    }
}