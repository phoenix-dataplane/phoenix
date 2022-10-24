#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

WORKDIR=`dirname $(realpath $0)`
cd $WORKDIR
sed -i 's/transport =\(.*\)/transport = "Rdma"/g' phoenix.toml
cargo rr --bin launcher -- -o ${OD} --benchmark ./launch_phoenix.toml --configfile ./config.toml --timeout 600
