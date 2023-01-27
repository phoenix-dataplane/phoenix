#pragma once

struct ValueRequest {
  int val;
};

struct ValueReply {
  int val;
};

ValueReply incrementServer(ValueRequest req);