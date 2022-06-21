# koala

[![Build Status](https://github.com/koalanet-project/koala/workflows/build/badge.svg)](https://github.com/koalanet-project/koala/actions)


Start the koala backend service on all servers.
```bash
KOALA_LOG=info cargo rr --bin koala
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

On another node, start the RPC client (assuming the server is on `rdma0.danyang-06`)
```bash
cargo rr --bin rpc_hello_client -- -c rdma0.danyang-06
```
