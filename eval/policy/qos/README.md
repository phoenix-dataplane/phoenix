```
numactl -N 0 -m 0 cargo run --release --bin rpc_bench_client -- -D 60 -p 5002 -i 1 --concurrency 64 --req-size 32768 --send-interval 500
```

```
numactl -N 0 -m 0 cargo run --release --bin rpc_bench_client -- -D 60 -p 5001 -i 1 --concurrency 1 --req-size 64 --log-latency --send-interval 100
```