#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

workdir=`dirname $(realpath $0)`
cd $workdir

# Run RDMA first
# concurrency = 32
sed -i 's/\(.*\)concurrency 1\(.*\)/\1concurrency 32\2/g' ../../../benchmark/benchmark/rpc_bench_rate/*.toml
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_rate --group rpc_bench_rate --configfile ./config.toml
rm -rf ${OD}/benchmark/rpc_bench_rate_rdma_32
mv ${OD}/benchmark/rpc_bench_rate ${OD}/benchmark/rpc_bench_rate_rdma_32
