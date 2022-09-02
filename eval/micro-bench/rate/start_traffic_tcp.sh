#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

WORKDIR=`dirname $(realpath $0)`
cd $WORKDIR

# Run TCP
sed -i 's/transport =\(.*\)/transport = "Tcp"/g' koala.toml
sed -i 's/\(.*\)concurrency 1\(.*\)/\1concurrency 32\2/g' ../../../benchmark/benchmark/rpc_bench_rate/*.toml
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_rate --group rpc_bench_rate --configfile ./config.toml
mv ${OD}/benchmark/rpc_bench_rate ${OD}/benchmark/rpc_bench_rate_tcp_32
