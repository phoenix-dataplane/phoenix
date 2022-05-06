#![feature(allocator_api)]
use std::time::Instant;

use libkoala::mrpc::alloc::Vec;
use libkoala::mrpc::codegen::{GreeterClient, HelloRequest};
use libkoala::mrpc::shared_heap::SharedHeapAllocator;

use smol;

const SERVER_ADDR: &str = "192.168.211.194";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = GreeterClient::connect((SERVER_ADDR, SERVER_PORT))?;
    smol::block_on(async {
        let mut reqs = vec![];
        for _ in 0..256 {
            let mut name = Vec::with_capacity_in(1000000, SharedHeapAllocator);
            name.resize(1000000, 42);
            reqs.push(name);
        }
        let start = Instant::now();
        for i in 0..256 {
            let req = Box::new_in(HelloRequest { name: reqs.swap_remove(0) }, SharedHeapAllocator);
            let resp = client.say_hello(req).await.unwrap();
            // eprintln!("resp {}: {:?}", i, resp);
            eprintln!("resp {} received, len: {}", i, resp.name.len());
        }
        let dura = start.elapsed();
        eprintln!("dura: {:?}, speed: {:?}", dura, 8e-9 * 256.0 * 1e6 / dura.as_secs_f64());
    });
    Ok(())
}
