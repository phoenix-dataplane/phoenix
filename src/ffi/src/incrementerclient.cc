#include "../include/incrementerclient.h"

IncrementerClient::IncrementerClient(CompletionConnectBridge conn_resp) {
  conn = conn_resp;
}

IncrementerClient* IncrementerClient::connect(const char* addr) {
  // first send connect commaned
  send_cmd_connect(addr);
  rust::Vec<RawFd> fds = recv_fds();

  CompletionConnectBridge conn_resp = recv_comp_connect();

  // assumption: number of fds is the same as number of readregions
  for (size_t i = 0; i < conn_resp.regions.size(); i++) {
    mmap((void*) conn_resp.regions[i].remote_addr, 
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
  recv_comp_mapped_addrs();

  update_protos();

  return new IncrementerClient(conn_resp);
}

ValueReply IncrementerClient::increment(ValueRequest req) {
   // allocate write region
  AllocShmCompletionBridge alloc_comp = allocate_shm(100, 8);
  std::cout << "after alloc" << std::endl;
  size_t* val = (size_t*) alloc_comp.remote_addr;

  mmap(val, 100, PROT_READ | PROT_WRITE, MAP_SHARED 
                                       | MAP_NORESERVE 
                                       | MAP_POPULATE 
                                       | MAP_FIXED_NOREPLACE, 
                                       (size_t) alloc_comp.fd, 
                                       (off_t) alloc_comp.file_off);

  if (fcntl(alloc_comp.fd, F_GETFD) == -1) {
    std::cout << std::strerror(errno) << std::endl;
  }
  // write int to rpc
  *val = req.val;

  // set up metadata for msg send
  CallIDBridge callid;
  callid.id = 1;

  MessageMetaBridge meta;
  meta.conn_id = conn.conn_handle;
  meta.service_id = 2056765301;
  meta.func_id = 3784353755; 
  meta.call_id = callid;
  meta.token = 1;
  meta.msg_type = RpcMsgTypeBridge::Request;

  MessageBridge msg;
  msg.meta = meta;
  msg.shm_addr_app = (size_t) val;
  msg.shm_addr_backend = (size_t) val;
  
  WorkRequestBridge wr;
  wr.wr_type = WorkRequestType::Call;
  wr.message = msg;
 
  // enqueue message
  enqueue_wr(wr);
  std::cout << "after enqueue" << std::endl;

  // receive reply
  MessageBridge ret = block_on_reply();
  ValueReply reply;
  reply.val = *((size_t*) ret.shm_addr_app);
  return reply;
}