# Working with mRPC Library
This tutorial describes how to develop user applications with mRPC library.

## Overview
mRPC library provides interfaces similar to [tonic](https://github.com/hyperium/tonic), which is a Rust implementation of gRPC.
mRPC Rust library is built to have first class support of the async ecosystem.
Protocol Buffers, which are used by gRPC to specify services and messages, are also applied by mRPC.
mRPC library's codegen generates client and server stubs based on the protobuf specs.

## Defining the HelloWorld service
The first step is to define a mRPC service with protobuf.
Let us first create a new Cargo project for our hello world demo.
```
cargo new rpc_hello
```

Then, let's create the `.proto` file for the server and client to use.
```
cd rpc_hello
mkdir proto
touch proto/rpc_hello.proto
```

Now, we define a simple hello world service:
```
syntax = "proto3";

package rpc_hello;

// The greeting service definition.
service Greeter {
    // Sends a greeting
    rpc SayHello (HelloRequest) returns (HelloReply) {}
}

// The request message containing the user's name.
message HelloRequest {
    bytes name = 1;
}

// The response message containing the greetings
message HelloReply {
    bytes message = 1;
}
```

Here we defined a `Greater` service, with `SayHello` RPC call. `SayHello` takes a `HelloRequest`, which contains a series of bytes,
and returns a `HelloReply` of bytes. Now, our `.proto` file is ready to go.

## Setup applications
We setup `Cargo.toml` to include mRPC library and other required dependencies, and specify a client binary and a server binary to compile.
```
[package]
name = "rpc_hello"
version = "0.1.0"
edition = "2021"

# See more keys and their definitions at https://doc.rust-lang.org/cargo/reference/manifest.html

[build-dependencies]
mrpc-build = { path = "../../mrpc/mrpc-build" }

[dependencies]
mrpc = { path = "../../mrpc" }
prost = { path = "../../3rdparty/prost", features = ["mrpc-frontend"] }
structopt = "0.3.23"
smol = "1.2.5"

[[bin]]
name = "rpc_hello_client"
path = "src/client.rs"

[[bin]]
name = "rpc_hello_server"
path = "src/server.rs"
```

## Generate client and server stubs
Before implementing the client and server, we need to write a build script to invoke `mrpc-build` to compile client and server stubs.
Create a `build.rs` in `rpc_hello/` with the following code:
```rust
const PROTO: &str = "proto/rpc_hello.proto";
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={PROTO}");
    mrpc_build::compile_protos(PROTO)?;
    Ok(())
}
```

## Writing the server
Create a file `server.rs`. We can start by including our hello world proto's messages types and server stub:
```rust
pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
}

use rpc_hello::greeter_server::{Greeter, GreeterServer};
use rpc_hello::{HelloReply, HelloRequest};
```

Now, we need to implement the `Greeter` service we defined.
In particular, we need to implement how `say_hello` should be handled by the server.
```rust
#[mrpc::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
    &self,
    request: RRef<HelloRequest>,
        ) -> Result<WRef<HelloReply>, mrpc::Status> {
            eprintln!("request: {:?}", request);

            let message = format!("Hello {}!", String::from_utf8_lossy(&request.name));
            let reply = WRef::new(HelloReply {
                message: message.as_bytes().into(),
            });

            Ok(reply)
        }
    }
```

Here, `RRef<HelloRequest>` wraps the received `HelloRequest` on the receive heap of the shared memory with the mRPC service.
`RRef` provides read-only access to the wrapped message. Copy-on-write can be performed if the application wants to modify a received message.
`WRef`, wraps a RPC message to be sent on the send heap of the shared memory.

With Greeter service implemented, we can implement a simple server that runs our Greeter service with smol runtime (which is a small and fast async runtime).
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    smol::block_on(async {
        let mut server = mrpc::stub::LocalServer::bind("0.0.0.0:5000")?;
        server
			.add_service(GreeterServer::new(MyGreeter::default()))
			.serve()
			.await?;
        Ok(())
    })
}
```

The complete code for the server is:
```rust
pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
}

use rpc_hello::greeter_server::{Greeter, GreeterServer};
use rpc_hello::{HelloReply, HelloRequest};

use mrpc::{RRef, WRef};

#[derive(Debug, Default)]
struct MyGreeter;

#[mrpc::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
		&self,
		request: RRef<HelloRequest>,
    ) -> Result<WRef<HelloReply>, mrpc::Status> {
        println!("request: {:?}", request);

        let message = format!("Hello {}!", String::from_utf8_lossy(&request.name));
        let reply = WRef::new(HelloReply {
            message: message.as_bytes().into(),
        });

        Ok(reply)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    smol::block_on(async {
        let mut server = mrpc::stub::LocalServer::bind("0.0.0.0:5000")?;
        server
			.add_service(GreeterServer::new(MyGreeter::default()))
			.serve()
			.await?;
        Ok(())
    })
}
```

## Writing the client
We can write a client to send a single request to server and get the reply.
```rust
pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
}

use rpc_hello::greeter_client::GreeterClient;
use rpc_hello::HelloRequest;


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = GreeterClient::connect("server-addr:5000")?;
    let req = HelloRequest {
        name: "mRPC".into(),
    };
    let reply = smol::block_on(client.say_hello(req))?;
    println!("reply: {}", String::from_utf8_lossy(&reply.message));
    Ok(())
}
```

With the client stub, we can just directly call `client.say_hello(req)` to send a request.
It will return a Future that we can await on, which resolves to a `Result<RRef<HelloReply>, mrpc::Status>`.
The `HelloRequest` and `HelloReply` types in the generated code internally
uses our customized Rust collection types, e.g., `mrpc::alloc::Vec<u8>`, where
it provides similar API as its std counterpart but directly allocates buffers on shared memory.

## Running the demo
First, start mRPC services on the machines that we will run the client and the server:
```
cd experimental/mrpc
cat load-mrpc-plugins.toml >> ../../phoenix.toml
cargo make
```

Note that mRPC service needs to run on both the client side and the server side.
After spinning up mRPC services, we can run the client and server applications:
```
cd experimental/mrpc/examples/rpc_hello
cargo run --release --bin rpc_hello_server
cargo run --release --bin rpc_hello_client
```
