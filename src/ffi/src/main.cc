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

  CompletionConnectBridge conn_resp = recv_comp_connect();

  // assumption: number of fds is the same as number of readregions
  for (size_t i = 0; i < conn_resp.regions.size(); i++) {
    void* read_region = mmap((void*) conn_resp.regions[i].remote_addr, 
                              conn_resp.regions[i].nbytes, 
                              PROT_READ | PROT_WRITE, 
                              MAP_SHARED 
                            | MAP_NORESERVE 
                            | MAP_POPULATE 
                            | MAP_FIXED_NOREPLACE, 
                            (size_t) fds[i].fd, (off_t) conn_resp.regions[i].file_off); 
    
    if (fcntl(fds[i].fd, F_GETFD) == -1) {
      std::cout << std::strerror(errno) << std::endl;
    }
  }

  send_cmd_mapped_addrs(conn_resp.conn_handle, conn_resp.regions);
  CompletionMappedAddrsBridge comp = recv_comp_mapped_addrs();
  std::cout << "mappedaddrs comp success: " << comp.success << std::endl;

  std::cout << "sending update protos" << std::endl;
  update_protos();
  // TODO: (amanm4) - error propogation

  // allocate write region
  AllocShmCompletionBridge alloc_comp = allocate_shm(100, 8);
  std::cout << "allocate success: " << alloc_comp.success << std::endl;
  size_t* val = (size_t*) alloc_comp.remote_addr;

  void* region = mmap(val, 100, PROT_READ | PROT_WRITE, MAP_SHARED 
                                                      | MAP_NORESERVE 
                                                      | MAP_POPULATE 
                                                      | MAP_FIXED_NOREPLACE, 
                                                      (size_t) alloc_comp.fd, 
                                                      (off_t) alloc_comp.file_off);

  if (fcntl(alloc_comp.fd, F_GETFD) == -1) {
    std::cout << std::strerror(errno) << std::endl;
  }

  // write int to rpc
  *val = 8;

  // set up metadata for msg send
  CallIDBridge callid;
  callid.id = 1;

  MessageMetaBridge meta;
  meta.conn_id = conn_resp.conn_handle;
  meta.service_id = 2056765301;
  meta.func_id = 3784353755; 
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
 
  // enqueue message
  ResultBridge resp = enqueue_wr(wr);
  std::cout << "enqueue_wr status: " << resp.success << std::endl;

  // receive reply
  MessageBridge ret = block_on_reply();
  std::cout << "response: ValueResponse { val: " << *((size_t*) ret.shm_addr_app) << " }" << std::endl;
}
