pub mod shmptr;

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


// unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for MessageTemplate<T> {
//     fn switch_address_space(&mut self) {
//         unsafe { self.val.as_mut() }.switch_address_space();
//         self.val = Unique::new(
//             self.val
//                 .as_ptr()
//                 .cast::<u8>()
//                 .wrapping_offset(query_shm_offset(self.val.as_ptr() as _))
//                 .cast(),
//         )
//         .unwrap();
//     }
// }

// // TODO(cjr): double-check if the code below is correct.
// unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for Vec<T> {
//     fn switch_address_space(&mut self) {
//         for v in self.iter_mut() {
//             v.switch_address_space();
//         }
//         // TODO(cjr): how to change the inner pointer of the Vec?
//         unsafe {
//             // XXX(cjr): the following operation has no safety guarantee.
//             // It's a very very dirty hack to overwrite the first 8 bytes of self.
//             let ptr = self as *mut _ as *mut isize;
//             let addr = ptr.read();
//             ptr.write(addr + query_shm_offset(addr as usize));
//         }
//         panic!("make sure switch_address_space is not calling this for HelloRequest or HelloReply");
//     }
// }

// // TODO(cjr): double-check if the code below is correct.
// unsafe impl<T> SwitchAddressSpace for Vec<T> {
//     default fn switch_address_space(&mut self) {
//         // TODO(cjr): how to change the inner pointer of the Vec?
//         eprintln!("make sure switch_address_space is calling this for HelloRequest");
//         unsafe {
//             // XXX(cjr): the following operation has no safety guarantee.
//             // It's a very very dirty hack to overwrite the first 8 bytes of self.
//             let ptr = self as *mut _ as *mut isize;
//             let addr = ptr.read();
//             ptr.write(addr + query_shm_offset(addr as usize));
//         }
//     }
// }
