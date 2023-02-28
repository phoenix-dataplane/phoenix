#!/usr/bin/env bash

PHOENIX_COMPILE_LOG=phoenix_compile_log.txt
HOST_DEP=target/host_dep

# cargo build -vv -r -p phoenix_common --color=always |& tee phoenix_compile_log.txt
cargo build -vv -r -p phoenixos --color=always |& tee $PHOENIX_COMPILE_LOG

cp -r target/release/deps $HOST_DEP

# DEP_FILE=`find target/release/deps | grep 'phoenix_common-[^.]*\.d'`
# echo $DEP_FILE
# cargo rr --bin phoenix_cargo -- --phoenix-dep $DEP_FILE --prebuilt-dir target/release/deps -- build -v --target-dir target --manifest-path experimental/mrpc/Cargo.toml --workspace

cargo rr --bin phoenix_cargo -- --compile-log $PHOENIX_COMPILE_LOG --host-dep $HOST_DEP -- build -v -r --target-dir target --manifest-path experimental/mrpc/Cargo.toml --workspace |& tee /tmp/f1.txt
cargo rr --bin phoenix_cargo -- --compile-log $PHOENIX_COMPILE_LOG --host-dep $HOST_DEP -- build -v -r |& tee /tmp/f2.txt
