#pragma once

struct ValueRequest {
  int val;
};

struct ValueReply {
  int val;
};

class CPPIncrementer {
    public:
    int highestReqSeen;
    ValueReply incrementServer(ValueRequest req);
};