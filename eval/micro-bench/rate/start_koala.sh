#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

workdir=`dirname $(realpath $0)`
cd $workdir

cargo rr --bin launcher -- -o ${OD} --benchmark ./launch_koala.toml --configfile ./config.toml --timeout 600
