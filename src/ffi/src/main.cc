#include "ffi/src/main.rs"
#include "rust/cxx.h"
#include <string>
#include <vector>
#include <iostream>

int main() {
  // first send connect commaned
  std::cout << "sending connect" << std::endl;
  send_cmd_connect("127.0.0.1:5000");
  rust::Vec<RawFd> fds = recv_fds();
   
  std::cout << "fds from recv_fds" << std::endl;
  std::cout << fds.front().fd << std::endl;

  CompletionConnectBridge conn_resp = recv_comp_connect();
  std::cout << "conn_resp success, regions size, first region handle, conn_handle " << std::endl;
  std::cout << conn_resp.success << std::endl;
  std::cout << conn_resp.regions.size() << std::endl;
  std::cout << conn_resp.regions.front().handle.id << std::endl;
  std::cout << conn_resp.conn_handle.id << std::endl;

  send_cmd_mapped_addrs(conn_resp.conn_handle, conn_resp.regions);
 
  CompletionMappedAddrsBridge comp = recv_comp_mapped_addrs();
  std::cout << "mappedaddrs comp success: " << std::endl;
  std::cout << comp.success << std::endl;

  // fd field is invalid
  AllocShmCompletionBridge alloc_comp = allocate_shm(100, 8);
  std::cout << "allocate success: " << std::endl;
  std::cout << alloc_comp.success << std::endl;
}
