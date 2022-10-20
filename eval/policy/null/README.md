This folder contains scripts for null policy.
**Edit config.toml and phoenix.toml first**

1. Start receiver backend on `danyang-06`. Start sender backend on
   `danyang-05`.
```
cjr@danyang-05 $ ./start_phoenix_tcp.sh [/tmp/mrpc-eval]
```

2. Start the rpc_bench_server and rpc_bench_client
```
cjr@danyang-05 $ ./start_traffic_tcp.sh [/tmp/mrpc-eval]
```

3. Attach policy at the client and the server
```
cjr@danyang-05 $ python3 attach_policy.py [/tmp/mrpc-eval]
cjr@danyang-06 $ python3 attach_policy.py [/tmp/mrpc-eval]
```

3. Wait for finshing rpc_bench.

