pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
    // include!("../../../mrpc/src/codegen.rs");
}

use mrpc::stub::TransportType;
use rpc_hello::greeter_client::GreeterClient;
use rpc_hello::HelloRequest;
use std::{thread, time::Duration, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut setting = mrpc::current_setting();
    setting.module_config = Some("MrpcLB".into());
    setting.transport = TransportType::Tcp;
    mrpc::set(&setting);
    let client = GreeterClient::multi_connect(vec!["localhost:5000", "localhost:5001"])?;
    println!("Connected to server!");
    let mut apple_count = 0;
    let mut banana_count = 0;
    let mut last_print_time = Instant::now();
    let interval = Duration::from_secs(5);
    let mut i = 0;
    loop {
        i = i ^ 1;
        let mut req = HelloRequest {
            name: "Apple".into(),
        };
        if i == 0 {
            req = HelloRequest {
                name: "Banana".into(),
            };
        }
        let reply = smol::block_on(client.say_hello(req));
        match reply {
            Ok(reply) => {
                let reply = String::from_utf8_lossy(&reply.message);
                if reply == "Hello Apple!" {
                    apple_count += 1;
                } else {
                    banana_count += 1;
                }
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }

        thread::sleep(Duration::from_millis(250));

        let elapsed = Instant::now().duration_since(last_print_time);
        if elapsed >= interval {
            println!(
                "Apple count: {}, Banana count: {}",
                apple_count, banana_count
            );
            last_print_time = Instant::now();
        }
    }
}
