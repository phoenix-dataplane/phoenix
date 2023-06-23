cargo run --release -p rpc_bench --bin rpc_bench_server -- --transport tcp 

cargo run --release -p rpc_bench --bin rpc_bench_client -- --transport tcp -D 600 -i 1 --req-size 64 -c 127.0.0.1


cargo run --release --bin addonctl -- --config eval/policy/nofile-logging/attach.toml --pid <client_pid> --sid 1
cargo run --release --bin addonctl -- --config eval/policy/logging/attach.toml --pid <client_pid> --sid 1

cargo run --release --bin addonctl -- --config eval/policy/logging/detach.toml --pid <client_pid> --sid 1
cargo run --release --bin addonctl -- --config eval/policy/nofile-logging/attach.toml --pid <client_pid> --sid 1/users/banruo/phoenix/examples