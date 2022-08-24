#!/usr/bin/env bash

# concurrency = 32
sed -i 's/\(.*\)concurrency 1\(.*\)/\1concurrency 32\2/g' ../../../benchmark/benchmark/rpc_bench_rate/*.toml
cargo rr --bin launcher -- --output-dir /tmp/mrpc-eval --benchmark ../../../benchmark/benchmark/rpc_bench_rate --group rpc_bench_rate --configfile ./config.toml
mv /tmp/mrpc-eval/benchmark/rpc_bench_rate /tmp/mrpc-eval/benchmark/rpc_bench_rate_32
