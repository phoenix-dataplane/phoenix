all: bench_lat bench_bw

bench_lat: bench_lat.cc bench_send_lat.h bench_read_lat.h bench_write_lat.h bench_util.h get_clock.cc get_clock.h
	g++ -Wall bench_lat.cc get_clock.cc -O3 -libverbs -lrdmacm -o bench_lat --std=c++11

bench_bw: bench_bw.cc bench_send_bw.h bench_read_bw.h bench_write_bw.h bench_util.h get_clock.cc get_clock.h
	g++ -Wall bench_bw.cc get_clock.cc -O3 -libverbs -lrdmacm -o bench_bw --std=c++11
