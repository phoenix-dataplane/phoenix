#!/usr/bin/env bash

# start the traffic
cargo rr --bin launcher -- -o /tmp/mrpc-eval --benchmark ./rpc_bench_tput_32b.toml --configfile ./config.toml
