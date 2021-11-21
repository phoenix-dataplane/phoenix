all: bench_lat bench_bw

bench_lat: bench_lat.c bench_send_lat.h bench_read_lat.h bench_write_lat.h bench_util.h get_clock.c get_clock.h
	gcc -Wall bench_lat.c get_clock.c -O3 -libverbs -lrdmacm -o bench_lat

bench_bw: bench_bw.c bench_send_bw.h bench_read_bw.h bench_write_bw.h bench_util.h get_clock.c get_clock.h
	gcc -Wall bench_bw.c get_clock.c -O3 -libverbs -lrdmacm -o bench_bw
