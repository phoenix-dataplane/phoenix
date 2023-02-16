#include "../include/incrementerclient.h"

void sendRequest(IncrementerClient* client, ValueRequest req) {
  std::cout << "request: ValueRequest { val: " << req.val << " }" << std::endl;
  ValueReply reply = client->increment(req);
  std::cout << "response: ValueReply { val: " << reply.val << " }" << std::endl;
}

int main() {
  IncrementerClient* client_1 = IncrementerClient::connect("127.0.0.1:5000");
  IncrementerClient* client_2 = IncrementerClient::connect("127.0.0.1:5002");
  ValueRequest req1;
  req1.val = 1;

  ValueRequest req2;
  req2.val = 50;

  sendRequest(client_1, req1);
  sendRequest(client_2, req2);
  return 0;
}
