#include "ffi/src/client.rs"
#include <iostream>

void sendRequest(IncrementerClient* client, ValueRequest* req) {
  std::cout << "request: ValueRequest { val: " << req->val() << " }" << std::endl;
  ValueReply* reply = client->increment(rust::Box<ValueRequest>::from_raw(req)).into_raw();
  std::cout << "response: ValueReply { val: " << reply->val() << " }" << std::endl;
}

int main() {
  IncrementerClient* client_1 = connect("127.0.0.1:5000").into_raw();
  ValueRequest* req = new_value_request().into_raw();
  req->set_val(0);
  req->add_foo(22);
  req->add_foo(42);

  sendRequest(client_1, req);
  
  return 0;
}
