#pragma once

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