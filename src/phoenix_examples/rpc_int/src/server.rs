pub mod rpc_int {
    mrpc::include_proto!("rpc_int");
}

use rpc_int::incrementer_server::{Incrementer, IncrementerServer};
use rpc_int::{ValueRequest, ValueReply};

use mrpc::{RRef, WRef};

#[derive(Debug, Default)]
struct MyIncrementer;

#[mrpc::async_trait]
impl Incrementer for MyIncrementer {
    async fn increment(
        &self,
        request: RRef<ValueRequest>,
    ) -> Result<WRef<ValueReply>, mrpc::Status> {
        eprintln!("request: {:?}", request);

        let reply = WRef::new(ValueReply {
            val: request.val + 1,
        });

        Ok(reply)
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    smol::block_on(async {
        let mut server = mrpc::stub::LocalServer::bind("0.0.0.0:5000")?;
        server
            .add_service(IncrementerServer::new(MyIncrementer::default()))
            .serve()
            .await?;
        Ok(())
    })
}
