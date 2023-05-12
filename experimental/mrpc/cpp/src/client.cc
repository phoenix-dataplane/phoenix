#include "cpp/src/clientcodegen.rs.h"
#include <iostream>

void completeIncrement(types::ffi::ValueReply* reply) { // take raw valreply pointer not rust box wrapper
  std::cout << "response: ValueReply { val: " << reply->val() << " }" << std::endl;
}

void sendRequest(incrementer::IncrementerClient* client, types::ffi::ValueRequest* req) {
  std::cout << "request: ValueRequest { val: " << req->val() << " }" << std::endl;
  client->increment(rust::Box<types::ffi::ValueRequest>::from_raw(req), (int32_t*) &completeIncrement);
}

int main() {
  incrementer::initialize();
  incrementer::IncrementerClient* client_1 = incrementer::connect("127.0.0.1:5000").into_raw();

  for (int i = 0; i < 5; i++) {
    types::ffi::ValueRequest* req = types::ffi::new_value_request().into_raw();
    req->set_val(i);
    for (int i = 0; i < 10; i++) {
      req->add_foo(i);
    }
    sendRequest(client_1, req);
  }
  
  while(true) {}

  return 0;
}