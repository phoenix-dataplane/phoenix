#![no_main]

use mrpc::WRef;
use mrpc::stub::{ClientStub, NamedService};


#[cxx::bridge]
mod client_ffi {
    #[derive(Debug, Default, Copy, Clone)]
    pub struct ValueRequest {
        pub val: i32,
    }

    #[derive(Debug, Default, Copy, Clone)]
    pub struct ValueReply {
        pub val: i32,
    }

    extern "Rust" {
        type IncrementerClient;

        fn connect(dst: String) -> Result<Box<IncrementerClient>>;
        fn increment(self: &IncrementerClient, req: &ValueRequest) ->  Result<ValueReply>;
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
        req: &client_ffi::ValueRequest,
    ) ->  Result<client_ffi::ValueReply, ::mrpc::Status>
    {
        let f = self.increment_inner(req);
        let r = smol::block_on(f);          // Rust stub returns a future, right now c++ client blocks on each req, what does a better interface look like?
        match r {
            Ok(val) => Ok( *val ),
            Err(error) => Err(error),
        }
    }

    fn increment_inner(
        &self,
        req: &client_ffi::ValueRequest,
    ) -> impl std::future::Future<Output = Result<mrpc::RRef<client_ffi::ValueReply>, ::mrpc::Status>> + '_
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