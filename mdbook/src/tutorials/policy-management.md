# Policy Management
This tutorial describes how network administrators can apply policies to running applications.

First, start Phoenix service and load mRPC modules.
```
cargo make run
cargo run --release --bin upgrade -- --config experimental/mrpc/load-mrpc-plugins.toml
```

Then run the user applications. Here we use `rpc_bench` as the example.
The `rpc_bench_client` will run for 60 seconds and print the RPC rate
every second.
```
cd experimental/mrpc/
cargo run --release -p rpc_bench --bin rpc_bench_server
cargo run --release -p rpc_bench --bin rpc_bench_client -- -D 60 -i 1 --req-size 64 -c <server_addr>
```

In `phoenixctl/src/bin`, we have a series of utilities for network administrators to interact with mRPC service.
You can compile all the phoenix-cli tools by
```
cargo make build-phoenix-cli
```

To apply a policy to an application, we must first retrieve information regarding it in mRPC service.
`list` is a utility used to list all engines running in mRPC service, along with the corresponding user process
the engine serves. The administrator can simply run:
```
cargo run --release --bin list
```
It will output a summary of running engines like the following:
```
+---------+-----+---------+--------+------------------------------------+
| PID     | SID | Service | Addons | Engines                            |
+---------+-----+---------+--------+------------------------------------+
| 2012290 | 0   | Salloc  | None   | +----------+--------------+        |
|         |     |         |        | | EngineId | EngineType   |        |
|         |     |         |        | +----------+--------------+        |
|         |     |         |        | | 0        | SallocEngine |        |
|         |     |         |        | +----------+--------------+        |
+---------+-----+---------+--------+------------------------------------+
| 2012290 | 1   | Mrpc    | None   | +----------+---------------------+ |
|         |     |         |        | | EngineId | EngineType          | |
|         |     |         |        | +----------+---------------------+ |
|         |     |         |        | | 2        | MrpcEngine          | |
|         |     |         |        | +----------+---------------------+ |
|         |     |         |        | | 1        | TcpRpcAdapterEngine | |
|         |     |         |        | +----------+---------------------+ |
+---------+-----+---------+--------+------------------------------------+
```
The above listing tells us there is a single user application with PID 2012290. The application has
two engine subscriptions, one of it is the mRPC engine (MrpcEngine),
which handles sending and receiving of RPC messages on the application's behalf.
The other (SallocEngine) is for allocating shared memory.

Policies will be applied on the mRPC engine subscription, which has a subscription ID (SID) 1 here.
Each policy is implemented as an engine. To apply a policy, we need a descriptor file to specify
which policy engine to attach, where the policy engine is inserted, and the configuration of the policy (a configuration string).

For instance, to apply a rate limit policy, we have the following descriptor file:
```toml
addon_engine = "RateLimitEngine"
tx_channels_replacements = [
    ["MrpcEngine", "RateLimitEngine", 0, 0],
    ["RateLimitEngine", "TcpRpcAdapterEngine", 0, 0],
]
rx_channels_replacements = []
group = ["MrpcEngine", "TcpRpcAdapterEngine"]
op = "attach"
config_string = '''
requests_per_sec = 1000
bucket_size = 1000
'''
```
Here, we specify that the rate limit engine should be inserted between `MrpcEngine` and `TcpRpcAdapterEngine`.
We also specify the rate should be limited at 1000 requests per second.

Then to apply this rate limit policy to the application, the administrator can use `addonctl` utility, passing in
the descriptor file, PID and SID.
```
cargo run  --release --bin addonctl -- --config eval/policy/ratelimit/attach.toml --pid 2012290 --sid 1
```

Removing a policy can be achieved in a similar fashion. For the above rate limit policy, we have the following descriptor
file to detach the policy, which removes the `RateLimitEngine`:
```toml
addon_engine = "RateLimitEngine"
tx_channels_replacements = [
    ["MrpcEngine", "TcpRpcAdapterEngine", 0, 0],
]
rx_channels_replacements = []
op = "detach"
```

After the following command is executed, rate limit policy is no longer applied.
```
cargo run  --release --bin addonctl -- --config eval/policy/ratelimit/detach.toml --pid 2012290 --sid 1
```
