#include "../include/incrementerclient.h"

int main() {
  IncrementerClient* client = IncrementerClient::connect("127.0.0.1:5000");
  ValueRequest req;
  req.val = 42;
  std::cout << "request: ValueRequest { val: " << req.val << " }" << std::endl;
  ValueReply reply = client->increment(req);
  std::cout << "response: ValueReply { val: " << reply.val << " }" << std::endl;
}