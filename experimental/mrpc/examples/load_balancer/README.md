## Build the application

```bash
# In phoenix/experimental/mrpc
cargo build --release --workspace -p rpc_hello_frontend
```

## Run the application with client

```bash
cargo rr -p rpc_echo --bin rpc_echo_server
# In a seperate terminal
cargo rr -p rpc_echo --bin rpc_echo_client
```

## Run the application with frontend

```bash
cargo rr -p rpc_echo --bin rpc_echo_server
# In a seperate terminal
cargo rr -p rpc_echo --bin rpc_echo_frontend
# In a seperate terminal
curl http://127.0.0.1:7878/hello
```