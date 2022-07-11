#include "capi.h"

#ifdef __cplusplus
extern "C" {
#endif

void* mt_index_create() {
  return new MtIndex();
}

void mt_index_destroy(void* obj) {
  delete static_cast<MtIndex*>(obj);
}

void mt_index_setup(void* obj, void *ti) {
  static_cast<MtIndex*>(obj)->setup(static_cast<threadinfo_t*>(ti));
}

void mt_index_swap_endian(uint64_t* x) { *x = __bswap_64(*x); }

void mt_index_put(void* obj, size_t key, size_t value, void* ti) {
  static_cast<MtIndex*>(obj)->put(key, value, static_cast<threadinfo_t*>(ti));
}

bool mt_index_get(void* obj, size_t key, size_t* value, void* ti) {
  return static_cast<MtIndex*>(obj)->get(key, *value,
                                         static_cast<threadinfo_t*>(ti));
}

size_t mt_index_sum_in_range(void* obj, size_t cur_key, size_t range,
                             void* ti) {
  return static_cast<MtIndex*>(obj)->sum_in_range(
      cur_key, range, static_cast<threadinfo_t*>(ti));
}

#ifdef __cplusplus
}
#endif
