#ifndef CAPI_H_
#define CAPI_H_
#include <stdint.h>

#include "mt_index_api.h"

#ifdef __cplusplus
extern "C" {
#endif
// MtIndex API
void* mt_index_create();
void mt_index_destroy(void* obj);
void mt_index_setup(void* obj, void* ti);
void mt_index_put(void* obj, size_t key, size_t value, void* ti);
bool mt_index_get(void* obj, size_t key, size_t* value, void* ti);
size_t mt_index_sum_in_range(void* obj, size_t cur_key, size_t range, void* ti);

// threadinfo API
void* threadinfo_make(int purpose, int index);
enum threadinfo_purpose { TI_MAIN, TI_PROCESS, TI_LOG, TI_CHECKPOINT };

#ifdef __cplusplus
}
#endif

#endif  // CAPI_H_
