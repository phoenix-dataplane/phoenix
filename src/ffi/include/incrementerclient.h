#pragma once

#include "ffi/src/client.rs"
#include "increment.h"
#include "rust/cxx.h"
#include <string>
#include <vector>
#include <iostream>
#include <sys/mman.h>
#include <cerrno>
#include <cstring>
#include <fcntl.h>

class IncrementerClient {
 public:
  static IncrementerClient* connect(const char* addr);
  ValueReply increment(ValueRequest req);
  IncrementerClient(CompletionConnectBridge conn_resp);
  CompletionConnectBridge conn;
};