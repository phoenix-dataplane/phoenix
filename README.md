# mRPC Artifact Evaluation
The evaluation scripts for micro-benchmarks can be found in `eval/micro-bench/`
We have a benchmark launcher that `ssh` into different hosts for the evaluation to run,
start the mRPC service and the user applications.
Results printed on `stdout` on all participating hosts are collected during the run.

To run the micro-benchmarks, we need to modify launcher's config. For example, to run the throughput benchmark,
we need to first modify `eval/micro-bench/bandwidth/launch_koala.toml`:
```
name = "launch_phoenix"
description = "Launch phoenix daemon"
group = "launch_phoenix"
timeout_secs = 600

[[worker]]
host = [SERVER_HOST_NAME]
bin = "phoenix"
args = "-c eval/micro-bench/bandwidth/phoenix.toml"

[[worker]]
host = [CLIENT_HOST_NAME]
bin = "phoenix"
args = "-c eval/micro-bench/bandwidth/phoenix.toml"
dependencies = [0] # launch in order, no very necessary
```

Replace `[SERVER_HOST_NAME]` and `[CLIENT_HOST_NAME]` with the hosts to run the server and client application (and mRPC services).

Then we need to modify the benchmark configurations, which can be found in `benchmark/benchmark`. For the throughput benchmark,
we need to modify the config files in `benchmark/benchmark/rpc_bench_tput/`. For instance, modify
`benchmark/benchmark/rpc_bench_tput/rpc_bench_tput_2kb.toml` as follows:
```


name = "benchmark/rpc_bench_tput/rpc_bench_tput_2kb"
description = "Run rpc_bench benchmark"
group = "rpc_bench_tput"
timeout_secs = 15

[[worker]]
host = [SERVER_HOST_NAME]
bin = "rpc_bench_server"
args = ""

[[worker]]
host = [CLIENT_HOST_NAME]
bin = "rpc_bench_client"
args = "-c [SERVER_HOST_ADDR] --concurrency 128 --req-size 2048 -D 10 -i 1 -l info"
dependencies = [0]
```

Make sure the same `[SERVER_HOST_NAME]` and `[CLIENT_HOST_NAME]` have been put into `rpc_bench_tput_2kb.toml` and `launch_koala.toml`

We also need to replace the server and client host name in
- `eval/micro-bench/bandwidth/collect.py`
- `eval/micro-bench/bandwidth/start_traffic_rdma.sh`
- `eval/micro-bench/bandwidth/start_traffic_tcp.sh`

After we modified all configs and scripts, following these steps to run the benchmark:
1. Start mRPC services
```
cd eval/micro-bench/bandwidth
./start_koala.sh
```

2. Start RPC benchmark application
```
./start_traffic_rdma.sh
./start_traffic_tcp.sh
```

3. Collect and parse results:
```
python3 ./collect.py
```