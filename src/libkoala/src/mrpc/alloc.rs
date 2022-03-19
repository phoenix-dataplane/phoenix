use crate::mrpc::codegen::SwitchAddressSpace;

use crate::mrpc::shared_heap::SharedHeapAllocator;

pub type Vec<T> = std::vec::Vec<T, SharedHeapAllocator>;
pub type Box<T> = std::boxed::Box<T, SharedHeapAllocator>;

// TODO(cjr): double-check if the code below is correct.
unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for Vec<T> {
    fn switch_address_space(&mut self) {
        for v in self.iter_mut() {
            v.switch_address_space();
        }
        // TODO(cjr): how to change the inner pointer of the Vec?
        unsafe {
            // XXX(cjr): the following operation has no safety guarantee.
            // It's a very very dirty hack to overwrite the first 8 bytes of self.
            let ptr = self as *mut _ as *mut isize;
            let addr = ptr.read();
            ptr.write(addr + SharedHeapAllocator::query_shm_offset(addr as usize));
        }
        panic!("make sure switch_address_space is not calling this for HelloRequest or HelloReply");
    }
}

// TODO(cjr): double-check if the code below is correct.
unsafe impl<T> SwitchAddressSpace for Vec<T> {
    default fn switch_address_space(&mut self) {
        // TODO(cjr): how to change the inner pointer of the Vec?
        eprintln!("make sure switch_address_space is calling this for HelloRequest");
        unsafe {
            // XXX(cjr): the following operation has no safety guarantee.
            // It's a very very dirty hack to overwrite the first 8 bytes of self.
            let ptr = self as *mut _ as *mut isize;
            let addr = ptr.read();
            ptr.write(addr + SharedHeapAllocator::query_shm_offset(addr as usize));
        }
    }
}
