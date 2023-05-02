#pragma once
#include <iostream>
#include <pthread.h>
#include <string>
// #include "ffi/src/server.rs"

struct ValueReply;
struct ValueRequest;

class RPCService {};

class CPPIncrementer : public RPCService {
  public:
    uint64_t highestReqSeen;
    const ValueReply& incrementServer(const ValueRequest& req);
}; 

typedef struct Args {
    std::string IP;
    CPPIncrementer *incr;
} Args;

void *StartServer(void *args);

pthread_t run_async(Args* thread_args);