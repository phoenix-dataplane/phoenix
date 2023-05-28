#include <iostream>
#include "mrpc.h"


WValueReply* increment(RValueRequest* req) {
  std::cout << "seen a request" << std::endl;
  std::cout << "sending a response" << std::endl;

  WValueReply* rep = new_wvaluereply();
  wvaluereply_set_val(rep, rvaluerequest_val(req) + 1);
  return rep;
}

int main() {
  CPPIncrementer incr;

  incr.increment_impl = increment;

  LocalServer* srv = bind_mrpc_server("0.0.0.0:5000");
  add_incrementer_service(srv, incr);

  local_server_serve(srv);

  return 0;
}
