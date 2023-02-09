#include "../include/incrementerclient.h"

int main() {
  IncrementerClient* client = IncrementerClient::connect("127.0.0.1:5002");

  ValueRequest req1;
  req1.val = 42;
  std::cout << "request: ValueRequest 1 { val: " << req1.val << " }" << std::endl;
  ValueReply reply1 = client->increment(req1);
  std::cout << "response: ValueReply 1 { val: " << reply1.val << " }" << std::endl;

  ValueRequest req2;
  req2.val = 1;
  std::cout << "request: ValueRequest 2 { val: " << req2.val << " }" << std::endl;
  ValueReply reply2 = client->increment(req2);
  std::cout << "response: ValueReply 2 { val: " << reply2.val << " }" << std::endl;

  return 0;
}