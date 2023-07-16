If you want to measure latency

change `--concurrency 128` to `--concurrency 1` and append
`--log-latency` to the end of the args in `rpc_bench_tput_32b.toml`.
