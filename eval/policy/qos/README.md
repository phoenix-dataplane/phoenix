This folder contains scripts for QoS policy.
**Edit config.toml, phoenix_client.toml phoenix_server.toml first**

1. Start receiver backend on `danyang-06`. Start sender backend on
   `danyang-05`.
```
cjr@danyang-05 $ ./start_phoenix.sh [/tmp/mrpc-eval]
```

2. Run QoS
```
python3 run_qos.py [/tmp/mrpc-eval]
```

NOTE: In this benchmark, we need to start two client-server pairs.
Due to the implementation of process killing of benchmark launcher, an error will occur at the end,
as one of the benchmark launcher will kill the client/server processes of both the benchmarks.
Safe to ignore the errors.