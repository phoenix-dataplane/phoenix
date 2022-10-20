#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi
server="danyang-06"
if [[ $# -ge 2 ]]; then
    server=$2
fi

WORKDIR=$(dirname $(realpath $0))
cd $WORKDIR

for concurrency in 128; do
    sed -i 's/--concurrency [0-9]*/--concurrency '"$concurrency"'/g' ../../../benchmark/benchmark/rpc_bench_tput/*.toml
    # sed -i 's/-D [0-9]*/-D 10/g' ../../../benchmark/benchmark/rpc_bench_tput/*.toml
    # sed -i 's/timeout_secs = [0-9]*/timeout_secs = 15/g' ../../../benchmark/benchmark/rpc_bench_tput/*.toml
    sed -i 's/transport =\(.*\)/transport = "Tcp"/g' phoenix.toml
    timestamp=$(date +%s%N)

    for i in 128b 512b 2kb 8kb 32kb 128kb 512kb 1mb 2mb 8mb; do
        ssh ${server} "pkill -9 mpstat"
        pkill -9 mpstat

        fsrv="/tmp/rpc_bench_cpu_monitor_mrpc_tcp_server_tput_${i}${concurrency}c_${timestamp}"
        fcli="/tmp/rpc_bench_cpu_monitor_mrpc_tcp_client_tput_${i}${concurrency}c_${timestamp}"
        echo "cpu monitor output:" ${fcli} ${fsrv}
        ssh ${server} "nohup sh -c 'mpstat -P ALL -u 1 | grep --line-buffered all' >${fsrv} 2>/dev/null &"
        nohup sh -c 'mpstat -P ALL -u 1 | grep --line-buffered all' >${fcli} 2>/dev/null &

        cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/rpc_bench_tput/rpc_bench_tput_${i}.toml --configfile ./config.toml

        pkill -9 mpstat
        ssh ${server} "pkill -9 mpstat; cat ${fsrv}" >${OD}/benchmark/rpc_bench_tput/rpc_bench_tput_${i}/mpstat_server.out
        cat ${fcli} >${OD}/benchmark/rpc_bench_tput/rpc_bench_tput_${i}/mpstat_client.out
    done

    rm -rf ${OD}/benchmark/rpc_bench_tput_tcp_${concurrency}
    mv ${OD}/benchmark/rpc_bench_tput ${OD}/benchmark/rpc_bench_tput_tcp_${concurrency}
done
