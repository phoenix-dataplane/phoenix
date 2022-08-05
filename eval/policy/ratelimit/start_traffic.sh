#!/usr/bin/env bash

# start the traffic
cargo rr --bin launcher -- -o /tmp/output --benchmark ./rpc_bench_tput_32b.toml --configfile ./config.toml
