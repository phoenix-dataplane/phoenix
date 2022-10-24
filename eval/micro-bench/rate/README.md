This folder contains scripts for benchmarking small RPC rate and
scalability.
**Edit config.toml and phoenix.toml first**

1. Start backend
```
./start_phoenix.sh [/tmp/mrpc-eval]
```

2. Start traffic. This will traverse all the configs and generate
   results under `/tmp/mrpc-eval`.
```
./start_traffic_rdma.sh [/tmp/mrpc-eval]
./start_traffic_tcp.sh [/tmp/mrpc-eval] [danyang-06]
```

3. Collect and parse results. This will read into `/tmp/mrpc-eval` and
   parse the text output to csv for plotting figures.
```
python3 ./collect.py [/tmp/mrpc-eval]
```
