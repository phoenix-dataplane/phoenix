#include <iostream>
#include "mrpc.h"

struct rpc_state {
  uint64_t highest_req_seen;
};

WValueReply* increment(void* state, RValueRequest* req) {
  rpc_state* s = (rpc_state*) state;
  s->highest_req_seen = std::max(s->highest_req_seen, rvaluerequest_val(req));
  std::cout << "highest request seen: " << s->highest_req_seen << std::endl;
  std::cout << "[" << unsigned(rvaluerequest_key(req, 0));
  for (size_t i = 1; i < rvaluerequest_key_size(req); i++) {
    std::cout << ", " << unsigned(rvaluerequest_key(req, i));
  }
  std::cout << "]" << std::endl;
  std::cout << "done" << std::endl;

  WValueReply* rep = new_wvaluereply();

  wvaluereply_set_val(rep, rvaluerequest_val(req) + 1);

  return rep;
}

int main() {
  CPPIncrementer incr;
  rpc_state s;

  incr.state = &s;
  incr.increment_impl = increment;

  LocalServer* srv = bind_mrpc_server("0.0.0.0:5000");
  add_incrementer_service(srv, incr);

  local_server_serve(srv);

  return 0;
}