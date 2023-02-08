#include "../include/incrementerclient.h"

int main() {
  IncrementerClient* client = IncrementerClient::connect("127.0.0.1:5000");

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



  ValueRequest req3;
  req3.val = 100;
  std::cout << "request: ValueRequest 3 { val: " << req3.val << " }" << std::endl;
  ValueReply reply3 = client->increment(req3);
  std::cout << "response: ValueReply 3 { val: " << reply3.val << " }" << std::endl;
}