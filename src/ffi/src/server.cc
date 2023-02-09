#include "../include/increment.h"
#include "ffi/src/server.rs"

int main() {
    CPPIncrementer incr;
    Args thread_args_1;
    thread_args_1.IP = "0.0.0.0:5000";
    thread_args_1.incr = &incr;
    pthread_t p1 = run_async(&thread_args_1);


    CPPIncrementer incr2;
    Args thread_args_2;
    thread_args_2.IP = "0.0.0.0:5002";
    thread_args_2.incr = &incr;
    pthread_t p2 = run_async(&thread_args_2);

    pthread_join(p1, NULL);
    pthread_join(p2, NULL);
    return 0;
}
