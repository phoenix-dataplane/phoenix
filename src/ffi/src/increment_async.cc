#include "../include/increment.h"
#include "ffi/src/server.rs"

void *StartServer(void *args) {
    pthread_detach(pthread_self());
    Args* thread_args = (Args*) args;
    std::cout << "thread started and detatched for ip: " << thread_args->IP << std::endl;
    run(thread_args->IP, *(thread_args->incr));
    pthread_exit(NULL);
}

pthread_t run_server_async(Args* thread_args) {
    pthread_t ptid;
    pthread_create(&ptid, NULL, &StartServer, thread_args);
    return ptid;
}