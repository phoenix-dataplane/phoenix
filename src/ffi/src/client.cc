#include "ffi/src/client.rs"
#include <iostream>

void sendRequest(IncrementerClient* client, ValueRequest req) {
  std::cout << "request: ValueRequest { val: " << req.val << " }" << std::endl;
  ValueReply reply = client->increment(req);
  std::cout << "response: ValueReply { val: " << reply.val << " }" << std::endl;
}

int main() {
  IncrementerClient* client_1 = connect("127.0.0.1:5000").into_raw();
  IncrementerClient* client_2 = connect("127.0.0.1:5002").into_raw();
  ValueRequest req1;
  req1.val = 1;

  ValueRequest req2;
  req2.val = 1000;

  for (int i = 0; i < 100; i++) {
    sendRequest(client_1, req1);
    sendRequest(client_2, req2);
    req1.val++;
    req2.val++;
  }
  
  return 0;
}
