# koala

On server `rdma0.danyang-06`, start koala backend,
```
KOALA_LOG=trace cargo rr --bin koala
```

then run the examples
```
cargo rr --bin send_bw --
```


On server B, start koala backend,
```
KOALA_LOG=trace cargo rr --bin koala
```

then run the examples
```
cargo rr --bin send_bw -- -c rdma0.danyang-06
```
