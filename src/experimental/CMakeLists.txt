cmake_minimum_required(VERSION 3.0)

set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -O3")

# the executables
add_executable(bench_lamport_spsc examples/bench_lamport_spsc.cc)
add_dependencies(bench_lamport_spsc dpdk::dpdk)
target_link_libraries(bench_lamport_spsc
    dpdk::dpdk
)
