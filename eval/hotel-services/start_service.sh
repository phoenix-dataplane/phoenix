#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

WORKDIR=`dirname $(realpath $0)`
cd $WORKDIR
cargo rr --bin launcher -- -o ${OD} --benchmark ./hotel_services.toml --configfile ./config.toml --timeout 600
