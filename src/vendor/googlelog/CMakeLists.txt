# Resources:
#   - https://github.com/rsakamoto/dexter/blob/13bac2f372bf0beeba673187c39d657978d71890/ext/glog/CMakeLists.txt

CMAKE_MINIMUM_REQUIRED(VERSION 3.0)

include(ExternalProject)

ExternalProject_Add(googlelog
    GIT_REPOSITORY "https://github.com/google/glog"
    GIT_TAG "v0.4.0"
    PREFIX ${CMAKE_CURRENT_BINARY_DIR}
    BUILD_IN_SOURCE 1
    UPDATE_COMMAND ""
    CONFIGURE_COMMAND ./autogen.sh && ./configure
    BUILD_COMMAND make
    INSTALL_COMMAND ""
)

set(GLOG_INCLUDE_DIRS ${CMAKE_CURRENT_BINARY_DIR}/src/googlelog/src PARENT_SCOPE)
set(GLOG_LINK_DIRS ${CMAKE_CURRENT_BINARY_DIR}/src/googlelog/.libs PARENT_SCOPE)
