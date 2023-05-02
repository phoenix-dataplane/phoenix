#include "../include/increment.h"
#include "ffi/src/servercodegen.rs"

void *StartServer(void *args) {
    pthread_detach(pthread_self());
    Args* thread_args = (Args*) args;
    std::cout << "thread started and detached for ip: " << thread_args->IP << std::endl;
    run(thread_args->IP, *(thread_args->incr));
    pthread_exit(NULL);
}

pthread_t run_async(Args* thread_args) {
    pthread_t ptid;
    pthread_create(&ptid, NULL, &StartServer, thread_args);
    return ptid;
}