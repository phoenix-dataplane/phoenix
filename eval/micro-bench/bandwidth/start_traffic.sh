#!/usr/bin/env bash

# concurrency = 32
sed -i 's/\(.*\)concurrency 1\(.*\)/\1concurrency 32\2/g' ../../../benchmark/benchmark/rpc_bench_tput/*.toml
cargo rr --bin launcher -- --output-dir /tmp/mrpc-eval --benchmark ../../../benchmark/benchmark/rpc_bench_tput --group rpc_bench_tput --configfile ./config.toml
rm -rf /tmp/mrpc-eval/benchmark/rpc_bench_tput_32
mv /tmp/mrpc-eval/benchmark/rpc_bench_tput /tmp/mrpc-eval/benchmark/rpc_bench_tput_32

# concurrency = 1
sed -i 's/\(.*\)concurrency 32\(.*\)/\1concurrency 1\2/g' ../../../benchmark/benchmark/rpc_bench_tput/*.toml
cargo rr --bin launcher -- --output-dir /tmp/mrpc-eval --benchmark ../../../benchmark/benchmark/rpc_bench_tput --group rpc_bench_tput --configfile ./config.toml
rm -rf /tmp/mrpc-eval/benchmark/rpc_bench_tput_1
mv /tmp/mrpc-eval/benchmark/rpc_bench_tput /tmp/mrpc-eval/benchmark/rpc_bench_tput_1
