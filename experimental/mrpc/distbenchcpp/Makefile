client: src/lib.rs example/client.cc
	cargo build && cbindgen --output mrpc.h && mv mrpc.h example/mrpc.h && g++ -o ./target/client example/client.cc target/debug/libcppbindgen.so

bindgen: src/lib.rs
	cbindgen --output mrpc.h && mv mrpc.h example/mrpc.h