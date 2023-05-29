#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

struct IncrementerClient;

struct LocalServer;

/// A thread-safe reference-counting pointer to the read-only shared memory heap.
template<typename T = void>
struct RRef;

struct ValueReply;

struct ValueRequest;

/// A thread-safe reference-couting pointer to the writable shared memory heap.
template<typename T = void>
struct WRef;

using WValueRequest = WRef<ValueRequest>;

using RValueRequest = RRef<ValueRequest>;

using WValueReply = WRef<ValueReply>;

using RValueReply = RRef<ValueReply>;

struct CPPIncrementer {
  WValueReply *(*increment_impl)(RValueRequest*);
};

extern "C" {

WValueRequest *new_wvaluerequest();

uint64_t rvaluerequest_val(const RValueRequest *self);

uint8_t rvaluerequest_key(const RValueRequest *self, uintptr_t index);

uintptr_t rvaluerequest_key_size(const RValueRequest *self);

void rvaluerequest_drop(RValueRequest *self);

uint64_t wvaluerequest_val(const WValueRequest *self);

uint8_t wvaluerequest_key(const WValueRequest *self, uintptr_t index);

uintptr_t wvaluerequest_key_size(const WValueRequest *self);

void wvaluerequest_set_val(WValueRequest *self, uint64_t val);

void wvaluerequest_set_key(WValueRequest *self, uintptr_t index, uint8_t value);

void wvaluerequest_key_add_byte(WValueRequest *self, uint8_t value);

void wvaluerequest_drop(WValueRequest *self);

WValueReply *new_wvalueresponse();

uint8_t rvalueresponse_key(const RValueReply *self, uintptr_t index);

uintptr_t rvalueresponse_key_size(const RValueReply *self);

void rvalueresponse_drop(RValueReply *self);

uint8_t wvalueresponse_key(const WValueReply *self, uintptr_t index);

uintptr_t wvalueresponse_key_size(const WValueReply *self);

void wvalueresponse_set_key(WValueReply *self, uintptr_t index, uint8_t value);

void wvalueresponse_key_add_byte(WValueReply *self, uint8_t value);

void wvalueresponse_drop(WValueReply *self);

LocalServer *bind_mrpc_server(const char *addr);

void local_server_serve(LocalServer *l);

IncrementerClient *incrementer_client_connect(const char *dst);

void increment(const IncrementerClient *self,
               const WValueRequest *req,
               void (*callback)(const RValueReply*));

void add_incrementer_service(LocalServer *l, CPPIncrementer service);

} // extern "C"
