all: bench_lat bench_bw

bench_lat:
	gcc bench_lat.c get_clock.c -O3 -libverbs -lrdmacm

bench_bw:
	gcc bench_bw.c get_clock.c -O3 -libverbs -lrdmacm
