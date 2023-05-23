use cxx::{type_id, ExternType};
use mrpc::alloc::Vec;

#[derive(Debug, Default, Clone)]
pub struct ValueRequest {
    pub val: u64,
    pub key: ::mrpc::alloc::Vec<u8>,
}

fn new_value_request() -> Box<ValueRequest> {
    Box::new( ValueRequest {
        val: 0,
        key: Vec::new()
    })
}

impl ValueRequest {
    fn val(&self) -> u64 {
        self.val
    }

    fn set_val(&mut self, val: u64) {
        self.val = val;
    }

    fn key(&self, index: usize) -> u8 {
        self.key[index]
    }

    fn key_size(&self) -> usize {
        self.key.len()
    }

    fn set_key(&mut self, index: usize, value: u8) {
        self.key[index] = value;
    }

    fn add_foo(&mut self, value: u8) {
        self.key.push(value);
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct ValueReply {
    pub val: u64,
}

fn new_value_reply() -> Box<ValueReply> {
    Box::new(ValueReply { val: 0 })
}

impl ValueReply {
    fn val(&self) -> u64 {
        self.val
    }

    fn set_val(&mut self, val: u64) {
        self.val = val
    }
}
unsafe impl ExternType for ValueRequest {
    type Id = type_id!("types::ffi::ValueRequest");
    type Kind = cxx::kind::Opaque;
}

#[cxx::bridge(namespace = types::ffi)]
pub mod types_ffi {
    extern "Rust" {
        type ValueRequest;
        type ValueReply;

        fn new_value_request() -> Box<ValueRequest>;
        fn val(self: &ValueRequest) -> u64;
        fn set_val(self: &mut ValueRequest, val: u64);
        fn key(self: &ValueRequest, index: usize) -> u8;
        fn key_size(self: &ValueRequest) -> usize;
        fn set_key(self: &mut ValueRequest, index: usize, value: u8);
        fn add_foo(self: &mut ValueRequest, value: u8);

        fn new_value_reply() -> Box<ValueReply>;
        fn val(self: &ValueReply) -> u64;
        fn set_val(self: &mut ValueReply, val: u64);
    }
}