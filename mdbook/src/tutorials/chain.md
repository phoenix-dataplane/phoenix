
# chain policies 

This is the tutorial about how to chain network policies in `mRPC`.

We also show how to dynamically update those policies.

## Setup

First, we need to setup the environment. 
```bash
# run this in `phoenix/experimental/mrpc`
cargo make
cargo make run
# run this in `phoenix/`
cargo make build-phoenix-cli
cargo run --release --bin upgrade -- --config experimental/mrpc/load-mrpc-plugins.toml
```

We use `rpc_echo` as application. In our example, we use `rpc_echo_server` and `rpc_echo_client2`. You can run this application without any policy and see the result.

```bash
# run this in `phoenix/experimental/mrpc`
# use two terminal for server and client, respectively 
# start server before client
cargo run --release -p rpc_echo --bin rpc_echo_server
cargo run --release -p rpc_echo --bin rpc_echo_client2
# run this in `phoenix/`
# remember the pid of client and server, you will need it later
# you can also use pgrep to get the pid.
cargo run --release --bin list
```

We can see that if no policy is applied, `Apple Count` and `Banana Count` appears every 5 seconds, and numbers are roughly the same.That is because the client alternatively sends `Apple` and `Banana` to the server, and counts the reply of each type.

The request rate is approximately 4/s. While the rate of real application should be much higher, we use a small rate just for illustration.


## Policy Phase 1: 

First, let's attach RateLimit, Logging and ACL to the application.

The policy chain after phase 1 looks like:

`Client(mrpc)->RateLimit->Logging->TCP=[network]=TCP->ACL->(mrpc)Server`

Let's add those policies by running the following commands. Note that you must run those commands in given order. Althrough `mrpc` allows us to attach policies in any order, each order requires different `toml` configuration files. You should check those files for detail.

```bash
# run this in `phoenix/`
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/receiver_attach.toml --pid <server_pid> --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/ratelimit_attach.toml --pid <client_pid>  --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase1/logging_attach.toml --pid <client_pid>  --sid 1

# check the engine status
cargo run --release --bin list
```

You should wait for a few seconds after each command, and see the effect of each policy.

If we add acl policy, all `Apple` requests will be blocked.

If we add ratelimit policy, the request rate is limited to 1/s.

If we add logging policy, the detailed log about request and reply will be written in `/tmp/phoenix/log/..`


## Policy Phase 2:

To show that we can dynamically update policies, we move the ACL policy to client side.

The policy chain after phase 2 looks like:

`Client(mrpc)->RateLimit->Logging->ACL->TCP=[network]=TCP->(mrpc)Server`

To do that, we need to run the following command:

```bash
# run this in `phoenix/`
cargo run --release --bin addonctl -- --config eval/policy/chain/phase2/sender_attach.toml --pid <client_pid> --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase2/receiver_detach.toml --pid <server_pid> --sid 1

# check the engine status
cargo run --release --bin list
```

Normally, no visible change should be observed. That is because the policy chain is semantically the same. 

However, the ACL policy is indeed moved to client side.

## Detach

To cleanup, run the following commands.

```bash
# use client pid
cargo run --release --bin addonctl -- --config eval/policy/chain/phase3/logging_detach.toml --pid <client_pid> --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase3/sender_detach.toml --pid <client_pid> --sid 1
cargo run --release --bin addonctl -- --config eval/policy/chain/phase3/ratelimit_detach.toml --pid <client_pid> --sid 1
```

You can see we use different order to detach policies. You can use any order you want as long as the `toml` file is correct.

After detach those policies, the application should behave the same as before.
