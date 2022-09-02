This folder contains scripts for running large RPC bandwidth mircobenchmark.

1. Start backend
```
./start_koala.sh
```

2. Start traffic. This will traverse all the configs and generate
   results under `/tmp/mrpc-eval`.
```
./start_traffic_rdma.sh
./start_traffic_tcp.sh
```

3. Collect and parse results. This will read into `/tmp/mrpc-eval` and
   parse the text output to csv for plotting figures.
```
./collect.py
```
