cmake_minimum_required(VERSION 3.0)

set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -O3 -march=native -mtune=native")

# the executable
add_executable(sender sender.cc)
add_dependencies(sender googlelog)
target_link_libraries(sender
    glog
    dpdk::dpdk
)

add_executable(receiver receiver.cc)
add_dependencies(receiver googlelog)
target_link_libraries(receiver
    glog
    dpdk::dpdk
)
