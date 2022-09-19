#!/usr/bin/env bash
OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

WORKDIR=`dirname $(realpath $0)`
cd $WORKDIR

# MobileNet
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/parameter_server/mobile_net.toml --configfile ./config.toml

# EfficientNet
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/parameter_server/efficient_net.toml --configfile ./config.toml

# InceptionV3
cargo rr --bin launcher -- --output-dir ${OD} --benchmark ../../../benchmark/benchmark/parameter_server/inception_v3.toml --configfile ./config.toml

rm -rf ${OD}/benchmark/parameter_server_with_scheduler
mv ${OD}/benchmark/parameter_server ${OD}/benchmark/parameter_server_with_scheduler
