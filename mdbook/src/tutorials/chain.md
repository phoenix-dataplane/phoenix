
# chain policies 

## init

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
# use two sperate terminal
# you must run server first, idk why, though.
# /experimental/mrpc
cargo run --release -p rpc_bench --bin rpc_bench_server -- --transport=tcp
cargo run --release -p rpc_bench --bin rpc_bench_client -- -D 600 -i 1 --req-size 64 -c 127.0.0.1 --transport=tcp

cargo run --release -p rpc_echo --bin rpc_echo_server
cargo run --release -p rpc_echo --bin rpc_echo_client

cargo run --release -p hotel_reservation --bin hotel_reservation_server
cargo run --release -p hotel_reservation --bin hotel_reservation_client

```

## apply engine

```bash
cargo run --release --bin list

cargo run --release --bin addonctl -- --config eval/policy/chain/attach_first.toml --pid 287769 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/attach_second.toml --pid 287769 --sid 1
# if detach logging first
cargo run --release --bin addonctl -- --config eval/policy/chain/detach_first.toml --pid 287769 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/detach_second.toml --pid 287769 --sid 1

# if detach ratelimit first
cargo run --release --bin addonctl -- --config eval/policy/chain/detach_ratelimit.toml --pid 287769 --sid 1
```

## acl

```bash
cargo run --release --bin addonctl -- --config eval/policy/hello-acl/attach.toml --pid 858166 --sid 1

```


## notes

```
edges.push(ChannelDescriptor(
                sender_engine,
                receiver_engine,
                sender_idx,
                recevier_idx,
            ));
```