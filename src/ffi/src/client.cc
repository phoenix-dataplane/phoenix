#include "ffi/src/clientcodegen.rs"
#include <iostream>

void sendRequest(IncrementerClient* client, ValueRequest* req) {
  std::cout << "request: ValueRequest { val: " << req->val() << " }" << std::endl;
  client->increment(rust::Box<ValueRequest>::from_raw(req));
}

void receiveReply(rust::Box<ValueReply> reply) {
  ValueReply* reply = reply.into_raw();
  std::cout << "response: ValueReply { val: " << reply->val() << " }" << std::endl;
}

int main() {
  initialize();
  IncrementerClient* client_1 = connect("127.0.0.1:5000").into_raw();

  for (int i = 0; i < 5; i++) {
    ValueRequest* req = new_value_request().into_raw();
    req->set_val(i);
    for (int i = 0; i < 10; i++) {
      req->add_foo(i);
    }
    sendRequest(client_1, req);
  }
  
  while(true) {}

  return 0;
}