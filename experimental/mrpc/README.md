# mRPC: Remote Procedure Call as a Managed System Service

mRPC, a managed RPC service, is built on top of Phoenix. mRPC realizes a novel RPC architecture that decouples (un)marshalling from RPC libraries to a centralized system service.

Compared with traditional library + sidecar solutions such as gRPC + Envoy, mRPC applies network policies and observability features with both security and low performance overhead, i.e., with minimal data movement and no redundant (un)marshalling. The mechanism supports live upgrade of RPC bindings, policies, transports, and marshalling without disrupting running applications.

## Dynamically Load mRPC Plugins
You can dynamically load mRPC plugins to phoenixos. First, change
directory to the phoenix project's root workspace. We provide a set of phoenix-cli
utilities as admin tools under `src/phoenixctl`.
```bash
cargo run --release --bin upgrade -- --config experimental/mrpc/load-mrpc-plugins.toml
```

`load-mrpc-plugins.toml` specifies the modules and addons to load. You
can change the configuration of them. After MrpcEngine, RpcAdapter, and
TcpRpcAdapter are loaded, you can run mRPC example applications.
