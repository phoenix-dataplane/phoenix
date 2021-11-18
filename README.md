# koala

On server `rdma0.danyang-06`, start koala backend,
```
KOALA_LOG=trace cargo run --release --bin experimental
```

then run the examples
```
cargo run --release --bin send_bw --
```


On server B, start koala backend,
```
KOALA_LOG=trace cargo run --release --bin experimental
```

then run the examples
```
cargo run --release --bin send_bw -- -c rdma0.danyang-06
```
