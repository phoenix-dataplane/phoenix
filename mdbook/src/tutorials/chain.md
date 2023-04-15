
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

## chain policy in Client side

The overall engine chain is like this:

`Client(mrpc)->RateLimit->Logging->ACL->TCP=[network]=TCP->(mrpc)Server`

We use `rpc_bench` as application to see the effect of RateLimit.

```bash
# use two sperate terminal
# you must run server first, idk why, though.
# /experimental/mrpc
cargo run --release -p rpc_bench --bin rpc_bench_server -- --transport=tcp
cargo run --release -p rpc_bench --bin rpc_bench_client -- -D 600 -i 1 --req-size 64 -c 127.0.0.1 --transport=tcp
```

Let add those policies, note that you must run those commands in order. Althrough you can add/remove policies in any order, but the toml file need to be changed to archieve that.

```bash
cargo run --release --bin list
# use the pid of client!
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/ratelimit_attach.toml --pid 885670 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/logging_attach.toml --pid 885519 --sid 1
# if detach logging first
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/logging_detach.toml --pid 885519 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/ratelimit_detach.toml --pid 885519 --sid 1
```


```bash
cargo run --release -p rpc_echo --bin rpc_echo_server
cargo run --release -p rpc_echo --bin rpc_echo_client

cargo run --release -p hotel_reservation --bin hotel_reservation_server
cargo run --release -p hotel_reservation --bin hotel_reservation_client
```

## acl

```bash
cargo run --release --bin addonctl -- --config eval/policy/hello-acl/attach.toml --pid 1022560 --sid 1

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