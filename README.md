# phoenix

[![Build Status](https://github.com/phoenix-dataplane/phoenix/workflows/build/badge.svg)](https://github.com/phoenix-dataplane/phoenix/actions)

Clone the repo and its submodule.
```
$ git clone git@github.com:phoenix-dataplane/phoenix.git --recursive
or
$ git clone git@github.com:phoenix-dataplane/phoenix.git
$ git submodule update --init --recursive
```

Make sure you have libibverbs, librdmacm, libnuma, protoc, libclang, globally available.
```
# apt install libclang-dev libnuma-dev librdmacm-dev libibverbs-dev protobuf-compiler
```

Start the phoenix backend service on all servers.
```bash
PHOENIX_LOG=info cargo rr --bin phoenix
```

(Recommended) Run examples using benchmark launcher.
```bash
cd benchmark
cargo rr --bin launcher -- -b benchmark/rpc_hello.toml
```


Run the examples manually.
On node `rdma0.danyang-06`, start a RPC server.
```bash
cargo rr --bin rpc_hello_server --
```

On another node, start the RPC client (assuming the server is on `rdma0.danyang-06`, otherwise example code should be modified)
```bash
cargo rr --bin rpc_hello_client -- 
```
