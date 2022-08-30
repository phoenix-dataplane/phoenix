#!/usr/bin/env bash
WORKDIR=`dirname $(realpath $0)`
cd $WORKDIR
cargo rr --bin launcher -- -o /tmp/mrpc-eval --benchmark ./launch_koala.toml --configfile ./config.toml --timeout 600