#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

WORKDIR=$(dirname $(realpath $0))
cd $WORKDIR

# concurrency = 1
sed -i 's/transport =\(.*\)/transport = "Tcp"/g' phoenix.toml
sed -i 's/\"RpcAdapterEngine\"/\"TcpRpcAdapterEngine\"/g' attach.toml
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ./rpc_bench_latency_64b.toml --configfile ./config.toml
