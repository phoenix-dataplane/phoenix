// mimic the generated code of rust stub
// Manually writing all the generated code.

#![no_main]

use std::sync::Arc;

use mrpc::{WRef};
use mrpc::stub::{ClientStub, NamedService};

include!("typescodegen.rs");

#[cxx::bridge]
mod incrementer_ffi {
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

// CLIENT CODE

#[derive(Debug)]
pub struct IncrementerClient {
    stub: Arc<ClientStub>,
}

fn connect(dst: String) -> Result<Box<IncrementerClient>, ::mrpc::Error> {
    // Force loading/reloading protos at the backend
    update_protos()?;

    let stub = ClientStub::connect(dst).unwrap();
    Ok(Box::new(IncrementerClient { stub: Arc::clone(&stub) }))
}

fn update_protos() -> Result<(), ::mrpc::Error> {
    let srcs = [include_str!(
        "../../../src/phoenix_examples/proto/rpc_int/rpc_int.proto"
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
        let r = tokio::task::spawn_local(f);
        // match r {
        //     Ok(val) => Ok( Box::new(*val) ),
        //     Err(error) => Err(error),
        // }
        Ok(new_value_reply())
    }

    fn increment_inner(
        &self,
        req: Box<ValueRequest>,
    ) -> impl std::future::Future<Output = Result<mrpc::RRef<ValueReply>, ::mrpc::Status>>
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