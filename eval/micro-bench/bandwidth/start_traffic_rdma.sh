#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

WORKDIR=`dirname $(realpath $0)`
cd $WORKDIR

# concurrency = 32
sed -i 's/transport =\(.*\)/transport = "Rdma"/g' koala.toml
sed -i 's/\(.*\)concurrency 1\(.*\)/\1concurrency 32\2/g' ../../../benchmark/benchmark/rpc_bench_tput/*.toml
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_tput --group rpc_bench_tput --configfile ./config.toml
rm -rf ${OD}/benchmark/rpc_bench_tput_32
mv ${OD}/benchmark/rpc_bench_tput ${OD}/benchmark/rpc_bench_tput_rdma_32

# concurrency = 1
sed -i 's/transport =\(.*\)/transport = "Rdma"/g' koala.toml
sed -i 's/\(.*\)concurrency 32\(.*\)/\1concurrency 1\2/g' ../../../benchmark/benchmark/rpc_bench_tput/*.toml
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_tput --group rpc_bench_tput --configfile ./config.toml
rm -rf ${OD}/benchmark/rpc_bench_tput_1
mv ${OD}/benchmark/rpc_bench_tput ${OD}/benchmark/rpc_bench_tput_rdam_1
