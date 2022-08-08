#!/usr/bin/env bash

trap "trap - SIGTERM && kill -- -$$" SIGINT SIGTERM SIGHUP EXIT

rates=(
1000
50000
100000
200000
350000
500000
)

for rate in ${rates[@]}; do
	echo $rate
	cargo r --bin koalactl -- --new-rate $rate --new-bucket-size $rate
	sleep 5.0
done

wait
