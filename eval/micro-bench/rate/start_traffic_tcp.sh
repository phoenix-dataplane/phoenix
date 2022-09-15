#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi
host="danyang-6"
if [[ $# -ge 2 ]]; then
    host=$2
fi

workdir=`dirname $(realpath $0)`
cd $workdir

# concurrency = 32
sed -i 's/\(.*\)concurrency 1\(.*\)/\1concurrency 32\2/g' ../../../benchmark/benchmark/rpc_bench_rate/*.toml

fname='/tmp/rpc_bench_cpu_monitor_mrpc_tcp_'`date +%s%N`
ssh ${host} "nohup sh -c 'mpstat -P ALL -u 1 | grep --line-buffered all' >${fname} 2>/dev/null &"
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_rate/rpc_bench_tput_32b_1c.toml --configfile ./config.toml
ssh ${host} "pkill -9 mpstat; cat ${fname}" >${OD}/benchmark/rpc_bench_rate/rpc_bench_rate_32b_1c/mpstat.out


fname='/tmp/rpc_bench_cpu_monitor_mrpc_tcp_'`date +%s%N`
ssh ${host} "nohup sh -c 'mpstat -P ALL -u 1 | grep --line-buffered all' >${fname} 2>/dev/null &"
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_rate/rpc_bench_tput_32b_2c.toml --configfile ./config.toml
ssh ${host} "pkill -9 mpstat; cat ${fname}" >${OD}/benchmark/rpc_bench_rate/rpc_bench_rate_32b_2c/mpstat.out


fname='/tmp/rpc_bench_cpu_monitor_mrpc_tcp_'`date +%s%N`
ssh ${host} "nohup sh -c 'mpstat -P ALL -u 1 | grep --line-buffered all' >${fname} 2>/dev/null &"
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_rate/rpc_bench_tput_32b_4c.toml --configfile ./config.toml
ssh ${host} "pkill -9 mpstat; cat ${fname}" >${OD}/benchmark/rpc_bench_rate/rpc_bench_rate_32b_4c/mpstat.out


fname='/tmp/rpc_bench_cpu_monitor_mrpc_tcp_'`date +%s%N`
ssh ${host} "nohup sh -c 'mpstat -P ALL -u 1 | grep --line-buffered all' >${fname} 2>/dev/null &"
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_rate/rpc_bench_tput_32b_8c.toml --configfile ./config.toml
ssh ${host} "pkill -9 mpstat; cat ${fname}" >${OD}/benchmark/rpc_bench_rate/rpc_bench_rate_32b_8c/mpstat.out


rm -rf ${OD}/benchmark/rpc_bench_rate_tcp_32
mv ${OD}/benchmark/rpc_bench_rate ${OD}/benchmark/rpc_bench_rate_tcp_32
