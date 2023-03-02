# Policy Management
This tutorial shows how administers can apply policies to running applications.

First, start mRPC service and run the user applications.
```
KOALA_LOG=info cargo run --release --bin koala
cargo run --release --bin rpc_bench_client
```

In `phoenixctl/src/bin`, we have a series of utilizes for administers to interact with mRPC service.
To apply a policy to an application, we must first retrive information regarding it in mRPC service.
`list` is a utility used to list all engines running in mRPC service, along with the corresponding user process
the engine serves.
The administer can simply run:
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
two engine subscriptions, one is mRPC engine,
which handles sending and receiving of RPC messages on the application's behalf.
The other is salloc engine, which handles mRPC library's requests to allocate storage on the shared memory.

Policies will be applied on the mRPC engine subscription, which has a subscription ID (SID) 1 here.
Each policy is implemented as an engine. To apply a policy, we need a descriptor file to specify
which policy engine to attach, where the policy engine is inserted, and some other configurations of the policy.

For instance, to apply a rate limit policy, we have the following descriptor file:
```
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
Here, we speicfy that the rate limit engine should be inserted between `MrpcEngine` and `TcpRpcAdapterEngine`.
We also speicfy the rate should be limited at 1000 requests per second.

Then to apply this rate limit policy to the application, the administer can use `addonctl` utility, passing in
the descriptor file, PID and SID.
```
cargo run  --release --bin addonctl -- --config eval/policy/ratelimit/attach.toml --pid 2012290 --sid 1
```

Removing a policy can be achieved in a similar fashion. For the same rate limit policy, we have the following descriptor
file to detach the policy, which removes the `RateLimitEngine`:
```
addon_engine = "RateLimitEngine"
tx_channels_replacements = [
["MrpcEngine", "RpcAdapterEngine", 0, 0],
]
rx_channels_replacements = []
op = "detach"
```

After the following command is executed, rate limit policy is no longer applied.
```
cargo run  --release --bin addonctl -- --config eval/policy/ratelimit/detach.toml --pid 2012290 --sid 1

```