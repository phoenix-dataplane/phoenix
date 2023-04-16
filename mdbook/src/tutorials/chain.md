
# chain policies 

## init

Build the CLI, Start the control plane

```bash
# in mrpc folder
cargo make
cargo make run
# in phoenix folder
cargo make build-phoenix-cli
```

## chain policy in Client side

The overall engine chain is like this:

`Client(mrpc)->RateLimit->Logging->ACL->TCP=[network]=TCP->(mrpc)Server`

We use `rpc_echo` as application to see the effect of RateLimit.

Let add those policies, note that you must run those commands in order. Althrough you can add/remove policies in any order, it requires you to modify toml file.


```bash
# use two different terminal, start server before client
# remember the pid of client, you will need it later
cargo run --release --bin list
cargo run --release -p rpc_echo --bin rpc_echo_server
cargo run --release -p rpc_echo --bin rpc_echo_client2
```

### apply policy

```bash
cargo run --release --bin upgrade -- --config experimental/mrpc/load-mrpc-plugins.toml

# use the pid of client!
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/acl_attach.toml --pid 1590837 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/ratelimit_attach.toml --pid 1590837 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/logging_attach.toml --pid 1590837 --sid 1
```

### results

If we apply no policy, the request rate is about 4/s. Since I let thread sleep 1s after sending a request in `rpc_echo_client2`.

If we add acl policy, half of request will be blocked, becasue `mRPC` is not in the ACL list.

If we add ratelimit policy, the request rate is limited to 1/s, you can perceive that from the terminal output.

If we add logging policy, you can check the log in `/tmp/phoenix/log/..`

Note, in reality, the rate should be much higher.

## Move acl to server side

Now we move the acl policy to server side, so that we can see the effect of chain policy.

To preserve the sematics, we need to first add acl policy in server side, then remove it in client side.


```bash
# note that pid is different
cargo run --release --bin addonctl -- --config eval/policy/chain/phase2/receiver_attach.toml --pid 1590810 --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase2/sender_detach.toml --pid 1590837 --sid 1

```
## notes

```bash
# use two sperate terminal
# you must run server first, idk why, though.
# /experimental/mrpc
cargo run --release -p rpc_bench --bin rpc_bench_server -- --transport=tcp
cargo run --release -p rpc_bench --bin rpc_bench_client -- -D 600 -i 1 --req-size 64 -c 127.0.0.1 --transport=tcp

cargo run --release --bin upgrade -- --config experimental/mrpc/load-mrpc-plugins.toml
cargo run --release --bin list
cargo run --release --bin addonctl -- --config eval/policy/hello-acl-receiver/attach.toml --pid 1319128 --sid 1


```