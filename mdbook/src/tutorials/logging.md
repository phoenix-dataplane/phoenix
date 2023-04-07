
## logging policy 

Build the CLI, Start the control plane

```bash
cargo make run
cargo run --release --bin upgrade -- --config experimental/mrpc/load-mrpc-plugins.toml
```

```bash
# use two sperate terminal
# phoenix/
cargo run --release -p rpc_bench --bin rpc_bench_client -- -D 600 -i 1 --req-size 64 -c 127.0.0.1 --transport=tcp
cargo run --release -p rpc_bench --bin rpc_bench_server -- --transport=tcp
```


```bash
# pheonix/experimental/mrpc
cargo make run
cargo run --release --bin list
# get the pid of server, say 1467838
cargo run --release --bin addonctl -- --config eval/policy/logging/attach.toml --pid 2151109 --sid 1
```


```bash
# expect some thing like that from the control plane:
#[2023-04-06 06:02:22.497730  INFO plugin/policy/logging/src/engine.rs:135] Got a message: MessageMeta { conn_id: Handle(252), service_id: 4059748245, func_id: 3687134534, call_id: CallId(1193824), token: 0, msg_type: Response }
```