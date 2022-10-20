#!/usr/bin/env bash

trap "trap - SIGTERM && kill -- -$$" SIGINT SIGTERM SIGHUP EXIT

OD=/tmp/mrpc-eval
if [[ $# -ge 1 ]]; then
    OD=$1
fi

WORKDIR=$(dirname $(realpath $0))
cd $WORKDIR

# concurrency = 128
cargo rr --bin launcher -- --output-dir ${OD} --timeout=120 --benchmark ./hotel_reservation.toml --configfile ./config.toml &

sleep 30

LIST_OUTPUT="${OD}"/policy/list.json
cargo rr --bin list -- --dump "${LIST_OUTPUT}" # Need to specifiy PHOENIX_PREFIX
cat "${LIST_OUTPUT}"
ARG_PID=$(cat "${LIST_OUTPUT}" | jq '.[] | select(.service == "Mrpc") | .pid')
ARG_SID=$(cat "${LIST_OUTPUT}" | jq '.[] | select(.service == "Mrpc") | .sid')
echo $ARG_SID

sleep 1

cargo run --bin addonctl -- --config ./attach_tcp.toml --pid ${ARG_PID} --sid ${ARG_SID} # Need to specifiy PHOENIX_PREFIX

wait
