#![feature(allocator_api)]
use libkoala::mrpc::alloc::Vec;
use libkoala::mrpc::codegen::{GreeterClient, HelloRequest};
use libkoala::mrpc::shared_heap::SharedHeapAllocator;

use smol;

const SERVER_ADDR: &str = "192.168.211.162";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = GreeterClient::connect((SERVER_ADDR, SERVER_PORT))?;
    let mut name = Vec::new_in(SharedHeapAllocator);
    name.push(1);
    name.push(2);
    name.push(3);
    name.push(4);
    let req = Box::new_in(HelloRequest { name }, SharedHeapAllocator);
    smol::block_on(async {
        let resp = client.say_hello(req).await.unwrap();
        eprintln!("resp: {:?}", resp);
    });
    Ok(())
}
