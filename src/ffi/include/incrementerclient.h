#pragma once

#include "ffi/src/main.rs"
#include "rust/cxx.h"
#include <string>
#include <vector>
#include <iostream>
#include <sys/mman.h>
#include <cerrno>
#include <cstring>
#include <fcntl.h>

struct ValueRequest {
  int val;
};

struct ValueReply {
  int val;
};

class IncrementerClient {
 public:
  static IncrementerClient* connect(const char* addr);
  ValueReply increment(ValueRequest req);
  IncrementerClient(CompletionConnectBridge conn_resp);
  CompletionConnectBridge conn;
};