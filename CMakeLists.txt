# CMakeLists files in this project can
# refer to the root source directory of the project as ${HELLO_SOURCE_DIR} and
# to the root binary directory of the project as ${HELLO_BINARY_DIR}.
cmake_minimum_required(VERSION 3.0)
project(phoenix)

# for clang's compile_commands.json
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
set(CMAKE_CXX_COMPILER_ID "GNU")
set(CMAKE_CXX_STANDARD 17)

set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} ${CMAKE_CURRENT_SOURCE_DIR}/cmake)

# add_subdirectory(src/vendor/googlelog)
# include_directories(${GLOG_INCLUDE_DIRS})
# link_directories(${GLOG_LINK_DIRS})

# find_package(dpdk REQUIRED)

# Compile experimental code
add_subdirectory(src/experimental)

# masstree_analytics
add_subdirectory(src/phoenix_examples/masstree_analytics/libmt_index)
