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
or
$ git clone git@github.com:phoenix-dataplane/phoenix.git
$ git submodule update --init --recursive
```

2. Install required packages.
Make sure you have libibverbs, librdmacm, libnuma, protoc, libclang
available on your system.
In addition, you need to have rustup and cargo-make installed.
On ubuntu 22.04, you can install them using the following
command.
```
# apt install libclang-dev libnuma-dev librdmacm-dev libibverbs-dev protobuf-compiler
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
$ cargo install cargo-make
```

Alternatively, if you already have VS Code and Docker installed, you can click the badge above or [here](https://vscode.dev/redirect?url=vscode://ms-vscode-remote.remote-containers/cloneInVolume?url=https://github.com/phoenix-dataplane/phoenix) to get started.

3. Build and run phoenixos service.
```bash
$ cargo make
```
`cargo make` without specifying a build target will build the default
target, which is "dev-test-flow" by default. You can inspect and
customize the stages of this default workflow in `Makefile.toml`.

Optionally, you can manually execute each step in the dev-test-flow.

Note, once you have finished building, you can use `cargo make run` or
`cargo make run-phoenixos` to start the service.

Without any plugin, phoenixos itself is just an empty control plane. Next,
you can build and load some useful plugins and run a few user applications.

### Building mRPC
mRPC is the first experimental feature on Phoenix.
You can simply change directory to `experimental/mrpc` to build, 
deploy mRPC plugins, and run phoenixos in one command.

```bash
$ cat experimental/mrpc/load-mrpc-plugins.toml >> phoenix.toml
$ cd experimental/mrpc && cargo make
or equivalently
$ cargo make --cwd experimental/mrpc
```

For inter-host RPC in the following `rpc_hello` example, you may need at
least two machines. However, you can still run the client and server on the same
machine, communicating through the same instance of phoenixos.
Choose whichever test scenario that is most suitable for you.

To begin with, make sure phoenixos is started on all servers,

Then, update the destination address in `experimental/mrpc/examples/rpc_hello/src/client.rs`
to your server address.

Next, build the example by
```bash
$ cargo build --release --manifest-path experimental/mrpc/Cargo.toml --workspace -p rpc_hello
```

You could also build all the mRPC example applications using
```bash
$ cargo make --cwd experimental/mrpc build-mrpc-examples
```
Note: building phoenixos and its plugins requires the plugins to link
with a prebuilt set of phoenix crates. This is currently done by
`tools/phoenix_cargo` and the entire workflow is handled by
`cargo-make`. However, building user libraries and apps does not require that.
We can still use `cargo`.

### Running mRPC examples
Once `rpc_hello` is built, you can have two methods to start it.
1. (Recommended) To start the applications on multiple machines, we prepare
a launcher for this job.
```bash
$ cd benchmark
$ cargo rr --bin launcher -- --benchmark benchmark/rpc_hello.toml
```

2. Alternatively, you can run the examples manually by
```bash
$ cd experimental/mrpc
(server) $ cargo rr -p rpc_hello --bin rpc_hello_server
(client) $ cargo rr -p rpc_hello --bin rpc_hello_client
```

You can explore the set of mRPC user applications in
`experimental/mrpc/examples`.

## Learning More
[**Documentation**](https://phoenix-dataplane.github.io/)

## License
Phoenix is licensed under the [Apache-2.0 license](https://github.com/phoenix-dataplane/phoenix/blob/main/LICENSE).
