#![no_main]

use mrpc::WRef;
use mrpc::alloc::Vec;
use mrpc::stub::{ClientStub, NamedService};

#[derive(Debug, Default, Clone)]
struct ValueRequest {
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


#[cxx::bridge]
mod client_ffi {
    extern "Rust" {
        type ValueRequest;
        type ValueReply;
        type IncrementerClient;

        fn connect(dst: String) -> Result<Box<IncrementerClient>>;
        fn increment(self: &IncrementerClient, req: Box<ValueRequest>) ->  Result<Box<ValueReply>>;

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

#[derive(Debug)]
pub struct IncrementerClient {
    stub: ClientStub,
}

fn connect(dst: String) -> Result<Box<IncrementerClient>, ::mrpc::Error> {
    // Force loading/reloading protos at the backend
    update_protos()?;

    let stub = ClientStub::connect(dst).unwrap();
    Ok(Box::new(IncrementerClient { stub }))
}

fn update_protos() -> Result<(), ::mrpc::Error> {
    let srcs = [include_str!(
        "../../phoenix_examples/proto/rpc_int/rpc_int.proto"
    )];
    ::mrpc::stub::update_protos(srcs.as_slice())
}


impl IncrementerClient {
    fn increment(
        &self,
        req: Box<ValueRequest>,
    ) ->  Result<Box<ValueReply>, ::mrpc::Status>
    {
        let f = self.increment_inner(req);
        let r = smol::block_on(f);          // Rust stub returns a future, right now c++ client blocks on each req, what does a better interface look like?
        match r {
            Ok(val) => Ok( Box::new(*val) ),
            Err(error) => Err(error),
        }
    }

    fn increment_inner(
        &self,
        req: Box<ValueRequest>,
    ) -> impl std::future::Future<Output = Result<mrpc::RRef<ValueReply>, ::mrpc::Status>> + '_
    {
        let call_id = self.stub.initiate_call();
        // Fill this with the right func_id
        let func_id = 3784353755;

        let r = WRef::new(*req);    // TODO(nikolabo): Rust stub only writes RPC data once, directly to shm heap, we introduce an extra copy here, how to avoid?

        self.stub
            .unary(Self::SERVICE_ID, func_id, call_id, r)
    }
}

impl NamedService for IncrementerClient {
    const SERVICE_ID: u32 = 2056765301;
    const NAME: &'static str = "rpc_int.Incrementer";
}