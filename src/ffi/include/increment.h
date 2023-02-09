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

class CPPIncrementer {
    public:
    // state about current connection
    int highestReqSeen;

    ValueReply incrementServer(ValueRequest req);
};

typedef struct Args {
    std::string IP;
    CPPIncrementer *incr;
} Args;

void *StartServer(void *args);

pthread_t run_server_async(Args* thread_args);