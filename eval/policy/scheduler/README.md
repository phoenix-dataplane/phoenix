This folder contains scripts for running large RPC bandwidth mircobenchmark.
**Edit config.toml and koala.toml first**

1. Start backend and start traffic. This will traverse all the configs and generate
   results under `/tmp/mrpc-eval`.
```
./start_koala_with_scheduler.sh [/tmp/mrpc-eval]
./start_traffic_with_scheduler.sh [/tmp/mrpc-eval]
./start_koala_without_scheduler.sh [/tmp/mrpc-eval]
./start_traffic_without_scheduler.sh [/tmp/mrpc-eval]
```

2. Collect and parse results. This will read into `/tmp/mrpc-eval` and
   parse the text output to csv for plotting figures.
```
python3 ./collect.py [/tmp/mrpc-eval]
```
