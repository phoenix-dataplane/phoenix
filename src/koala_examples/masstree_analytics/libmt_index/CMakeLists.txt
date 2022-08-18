cmake_minimum_required(VERSION 3.0)
project(mt_index)

# for clang's compile_commands.json
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
set(CMAKE_CXX_COMPILER_ID "GNU")
set(CMAKE_CXX_STANDARD 17)

set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} ${CMAKE_CURRENT_SOURCE_DIR}/cmake)

# source files of libmt_index, don't recurse
FILE(GLOB SOURCE "*.cc")

message(STATUS "CMAKE_SOURCE_DIR: ${CMAKE_SOURCE_DIR}")
message(STATUS "PROJECT_SOURCE_DIR: ${PROJECT_SOURCE_DIR}")
message(STATUS "PROJECT_BINARY_DIR: ${PROJECT_BINARY_DIR}")
message(STATUS "PROJECT_ROOT: ${PROJECT_ROOT}")

# local variables
set(mt_index_INCLUDE_DIR_L "")
set(mt_index_LIBRARY_DIRS_L "")
set(mt_index_LINKER_LIBS_L "")

# masstree
add_subdirectory("${PROJECT_SOURCE_DIR}/masstree-beta")
list(APPEND mt_index_INCLUDE_DIR_L ${PROJECT_SOURCE_DIR}/masstree-beta)
list(APPEND mt_index_LIBRARY_DIRS_L ${PROJECT_BINARY_DIR}/masstree-beta)
list(APPEND mt_index_LINKER_LIBS_L masstree)
add_definitions(-include ${PROJECT_BINARY_DIR}/masstree-beta/config.h)

# mt_index include dir
list(APPEND mt_index_INCLUDE_DIR_L "${PROJECT_SOURCE_DIR}")

# the mt_index library
add_library(mt_index STATIC ${SOURCE})
add_dependencies(mt_index masstree)
include_directories(mt_index ${mt_index_INCLUDE_DIR_L})
link_directories(mt_index ${mt_index_LIBRARY_DIRS_L})
target_link_libraries(mt_index ${mt_index_LINKER_LIBS_L})

get_directory_property(hasParent PARENT_DIRECTORY)
if(hasParent)
    # exports
    list(APPEND mt_index_LINKER_LIBS_L mt_index)
    set(mt_index_LINKER_LIBS ${mt_index_LINKER_LIBS_L} PARENT_SCOPE)
    set(mt_index_INCLUDE_DIRS ${mt_index_INCLUDE_DIR_L} PARENT_SCOPE)
    set(mt_index_LIBRARY_DIRS "${PROJECT_BINARY_DIR}" PARENT_SCOPE)
    add_subdirectory(test)
else()
    # This line is needed. Cargo build script will rely on it later.
    install(TARGETS masstree DESTINATION .)
    install(TARGETS mt_index DESTINATION .)
endif()
