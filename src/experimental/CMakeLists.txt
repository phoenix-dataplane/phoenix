cmake_minimum_required(VERSION 3.0)

set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -O2")

# the executables
add_executable(bench_lamport_spsc examples/bench_lamport_spsc.cc)
target_link_libraries(bench_lamport_spsc pthread)

add_executable(bench_lat examples/bench_lat.cc examples/get_clock.cc)
target_link_libraries(bench_lat ibverbs rdmacm)

add_executable(bench_bw examples/bench_bw.cc examples/get_clock.cc)
target_link_libraries(bench_bw ibverbs rdmacm)
