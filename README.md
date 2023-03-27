<div align="center">
<img src="https://github.com/phoenix-dataplane/phoenix/blob/main/phoenix-logo-red-black.png" alt="logo" width="250"></img>
<br></br>
</div>

# Phoenix Dataplane Service

[![Build Phoenix](https://github.com/phoenix-dataplane/phoenix/actions/workflows/build-phoenix.yml/badge.svg)](https://github.com/phoenix-dataplane/phoenix/actions/workflows/build-phoenix.yml)
[![Build mRPC](https://github.com/phoenix-dataplane/phoenix/actions/workflows/build-mrpc.yml/badge.svg)](https://github.com/phoenix-dataplane/phoenix/actions/workflows/build-mrpc.yml)
[![Open in Dev Containers](https://img.shields.io/static/v1?label=Dev%20Containers&message=Open&color=blue&logo=visualstudiocode)](https://vscode.dev/redirect?url=vscode://ms-vscode-remote.remote-containers/cloneInVolume?url=https://github.com/phoenix-dataplane/phoenix)

[**Documentation**](https://phoenix-dataplane.github.io/)

Phoenix is a dataplane service which serves as a framework to develop and deploy various kinds of managed services.

The key features of Phoenix include:

**Modular Plugin System**: Phoenix provides an engine abstraction which, as the modular unit, can be developed, dynamically load, scheduled, and even be live upgraded with minimal disruption to user applications.

**High-performance Networking**: Phoenix offers managed access to networking devices while exposing a user-friendly API.

**Policy Manageability**: Phoenix supports application-layer policies which can be specified by infrastructure administers to gain visibility and control user application behaviors.

## Getting Started

### Building Phoenix
1. Clone the repo and its submodules.
```
$ git clone git@github.com:phoenix-dataplane/phoenix.git --recursive
```

2. Install required packages.
Make sure you have `libibverbs`, `librdmacm`, `libnuma`, `protoc`, `libclang`, and
`cmake` available on your system.
Additionally, you need to have `rustup` and `cargo-make` installed.
For Ubuntu 22.04, you can use the following commands:
```
$ sudo apt update
$ sudo apt install libclang-dev libnuma-dev librdmacm-dev libibverbs-dev protobuf-compiler cmake
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
$ cargo install cargo-make
```

Alternatively, if you already have VS Code and Docker installed, click the badge above or [here](https://vscode.dev/redirect?url=vscode://ms-vscode-remote.remote-containers/cloneInVolume?url=https://github.com/phoenix-dataplane/phoenix) to start.

3. Build and run PhoenixOS service.
```bash
$ cargo make
```
By default, cargo make will build the `dev-test-flow` target. You can inspect and
customize the stages of this workflow in `Makefile.toml`. Use `cargo make run-phoenixos` to start the service after building.

You can also manually execute each step in the dev-test-flow.

PhoenixOS without any plugins is just an empty control plane. Next,
you can build and load some useful plugins and run a few user applications.

### Building and Running mRPC
mRPC is the first experimental feature on Phoenix.
To build and deploy mRPC plugins, and run PhoenixOS, follow these steps:

```bash
$ cd experimental/mrpc
$ cat load-mrpc-plugins.toml >> ../../phoenix.toml
$ cargo make
```

<!-- For inter-host RPC in the following `rpc_hello` example, you may need at
least two machines. However, you can still run the client and server on the same
machine, communicating through the same instance of phoenixos.
Choose whichever test scenario that is most suitable for you. -->

Ensure that exactly one instance of PhoenixOS is running on each server.

Note: If you have multiple machines, update the destination address in `experimental/mrpc/examples/rpc_hello/src/client.rs`
to your server address.

Next, build the `rpc_echo` example:
```bash
$ cargo build --release --workspace -p rpc_echo
```

You can also build all mRPC examples using:
```bash
$ cargo make build-mrpc-examples
```
Note: building phoenixos and its plugins requires the plugins to link
with a prebuilt set of phoenix crates. This is currently done by
`tools/phoenix_cargo` and the entire workflow is handled by
`cargo-make`. However, building user libraries and apps does not require that.
We can still use `cargo`.

### Running mRPC examples

You can run the examples manually by
```bash
$ cargo rr -p rpc_echo --bin rpc_echo_server
# In a seperate terminal
$ cargo rr -p rpc_echo --bin rpc_echo_client
```

Note: If you have multiple machines, we provide a launcher to help with running the examples:
```bash
$ cd ../../benchmark
# Follow the README under benchmark directory and update config.toml
$ cargo rr --bin launcher -- --benchmark benchmark/rpc_echo.toml
``` 

You can explore the set of mRPC user applications in
`experimental/mrpc/examples`.

## Learning More
[**Documentation**](https://phoenix-dataplane.github.io/)

## License
Phoenix is licensed under the [Apache-2.0 license](https://github.com/phoenix-dataplane/phoenix/blob/main/LICENSE).