#pragma once
#include <iostream>
#include <pthread.h>
#include <string>

struct ValueRequest {
  int val;
};

struct ValueReply {
  int val;
};

class RPCService {};

class CPPIncrementer : public RPCService {
  public:
    int highestReqSeen;
    ValueReply incrementServer(ValueRequest req) {
      // lock
      this->highestReqSeen = std::max(this->highestReqSeen, req.val);
      std::cout << "highest request seen: " << this->highestReqSeen << std::endl;
      ValueReply rep;
      rep.val = req.val + 1;
      // release
      return rep;
    }
}; 

typedef struct Args {
    std::string IP;
    CPPIncrementer *incr;
} Args;

void *StartServer(void *args);

pthread_t run_async(Args* thread_args);