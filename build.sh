#!/usr/bin/env bash

set -euo pipefail
PHOENIX_COMPILE_LOG=phoenix_compile_log.txt
HOST_DEP=target/host_dep

# cargo build -vv -r -p phoenix_common --color=always |& tee phoenix_compile_log.txt
cargo build -vv -r -p phoenix_common --color=always |& tee $PHOENIX_COMPILE_LOG

cp -r target/release/deps $HOST_DEP

cargo build -r --bin phoenix_cargo
rm -r target/release/.fingerprint

target/release/phoenix_cargo --compile-log $PHOENIX_COMPILE_LOG --host-dep $HOST_DEP -- build -v -r --target-dir target --manifest-path experimental/mrpc/Cargo.toml --workspace |& tee /tmp/f1.txt
target/release/phoenix_cargo --compile-log $PHOENIX_COMPILE_LOG --host-dep $HOST_DEP -- build -v -r |& tee /tmp/f2.txt
