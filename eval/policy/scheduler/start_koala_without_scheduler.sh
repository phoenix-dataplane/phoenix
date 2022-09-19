#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

WORKDIR=`dirname $(realpath $0)`
cd $WORKDIR
sed -i 's/enable_scheduler =\(.*\)/enable_scheduler = false/g' koala.toml
cargo rr --bin launcher -- -o ${OD} --benchmark ./launch_koala.toml --configfile ./config.toml --timeout 600
