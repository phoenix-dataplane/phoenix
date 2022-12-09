pub mod rpc_int {
    mrpc::include_proto!("rpc_int");
}

use rpc_int::incrementer_client::IncrementerClient;
use rpc_int::ValueRequest;

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let client = IncrementerClient::connect("127.0.0.1:5000")?;
    let req = ValueRequest {
        val: 0,
    };
    let reply = smol::block_on(client.increment(req))?;
    println!("reply: {}", reply.val);
    // should print 1
    Ok(())
}
