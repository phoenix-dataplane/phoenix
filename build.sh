#!/usr/bin/env bash

cargo build -vv -r -p phoenix_common --color=always |& tee phoenix_compile_log.txt

DEP_FILE=`find target/release/deps | grep 'phoenix_common-[^.]*\.d'`
echo $DEP_FILE

# cargo rr --bin phoenix_cargo -- --phoenix-dep $DEP_FILE --prebuilt-dir target/release/deps -- build -v --target-dir target --manifest-path experimental/mrpc/Cargo.toml --workspace
cargo rr --bin phoenix_cargo -- --compile-log phoenix_compile_log.txt -- build -v --target-dir target --manifest-path experimental/mrpc/Cargo.toml --workspace
