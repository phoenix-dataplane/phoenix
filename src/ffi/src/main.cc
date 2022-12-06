#include "ffi/src/main.rs"
#include "rust/cxx.h"
#include <string>
#include <vector>
#include <iostream>
#include <sys/mman.h>
#include <cerrno>
#include <cstring>
#include <fcntl.h>

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

  std::cout << "sending update protos" << std::endl;
  update_protos();

  // fd field is invalid
  AllocShmCompletionBridge alloc_comp = allocate_shm(100, 8);
  std::cout << "allocate success: " << std::endl;
  std::cout << alloc_comp.success << std::endl;
  std::cout << (size_t) alloc_comp.fd << std::endl;
  size_t* val = (size_t*) alloc_comp.remote_addr;

  void* region = mmap(val, 100, PROT_READ | PROT_WRITE, MAP_SHARED 
                                                      | MAP_NORESERVE 
                                                      | MAP_POPULATE 
                                                      | MAP_FIXED_NOREPLACE, (size_t) alloc_comp.fd, (off_t) alloc_comp.file_off);

  if (fcntl(alloc_comp.fd, F_GETFD) == -1) {
    std::cout << std::strerror(errno) << std::endl;
  }

  std::cout << val << std::endl;
  *val = 8;

  CallIDBridge callid;
  callid.id = 1;

  MessageMetaBridge meta;
  meta.conn_id = conn_resp.conn_handle;
  meta.service_id = 2762047998;
  meta.func_id = 3933748028; 
  meta.call_id = callid;
  meta.token = 1;
  meta.msg_type = RpcMsgTypeBridge::Request;;

  MessageBridge msg;
  msg.meta = meta;
  msg.shm_addr_app = (size_t) val;
  msg.shm_addr_backend = (size_t) val;
  
  WorkRequestBridge wr;
  wr.wr_type = WorkRequestType::Call;
  wr.message = msg;
 
  ResultBridge resp = enqueue_wr(wr);
  std::cout << resp.success << std::endl;
}
