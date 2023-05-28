#include <iostream>
#include "mrpc.h"

void complete_increment(const RValueReply *reply) {
  std::cout << "response: ValueReply { val: " << rvaluereply_val(reply) << " }" << std::endl;
}

int main() {
  IncrementerClient* client = incrementer_client_connect("127.0.0.1:5000");

  for (int i = 0; i < 5; i++) {
    WValueRequest* req = new_wvaluerequest();
    wvaluerequest_set_val(req, i);
    for (int j = 0; j < 10; j++) {
      wvaluerequest_key_add_byte(req, i);
    }
    
    std::cout << "request: ValueRequest { val: " << wvaluerequest_val(req) << " }" << std::endl;
    increment(client, req, complete_increment);
  }

  while(true) {}

  return 0;
}