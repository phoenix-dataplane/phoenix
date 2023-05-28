client: lib bindings example/client.cc
	g++ -o target/client example/client.cc target/debug/libdistbench.so

server: lib bindings example/server.cc
	g++ -o target/server example/server.cc target/debug/libdistbench.so

bindings: src/lib.rs
	cbindgen --output mrpc.h .
	mv mrpc.h example/mrpc.h

lib: src/lib.rs
	cargo build

clean:
	cargo clean
	rm example/mrpc.h

.PHONY: client server bindings lib clean
