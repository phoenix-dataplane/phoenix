
## logging policy 

Build the CLI, Start the control plane

```bash
# in mrpc folder
cargo make
cargo make run
# in phoenix folder
cargo make build-phoenix-cli
cargo run --release --bin upgrade -- --config experimental/mrpc/load-mrpc-plugins.toml
```

```bash
cargo run --release --bin list
# don't forget to CHANGE the pid
cargo run --release --bin addonctl -- --config eval/policy/logging/attach.toml --pid 2151109 --sid 1
```

```bash
# use two sperate terminal
# you must run server first, idk why, though.
# /experimental/mrpc
cargo run --release -p rpc_bench --bin rpc_bench_server -- --transport=tcp
cargo run --release -p rpc_bench --bin rpc_bench_client -- -D 600 -i 1 --req-size 64 -c 127.0.0.1 --transport=tcp

```

```bash
# expect some thing like that from the control plane:
#[2023-04-06 06:02:22.497730  INFO plugin/policy/logging/src/engine.rs:135] Got a message: MessageMeta { conn_id: Handle(252), service_id: 4059748245, func_id: 3687134534, call_id: CallId(1193824), token: 0, msg_type: Response }
```

## to chain policies

```bash
cargo run --release --bin list

cargo run --release --bin addonctl -- --config eval/policy/chain/attach_first.toml --pid 263173 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/attach_second.toml --pid 263173 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/detach_first.toml --pid 263173 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/detach_second.toml --pid 263173 --sid 1


```