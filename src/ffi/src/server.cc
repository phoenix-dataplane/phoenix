#include "../include/increment.h"
#include "ffi/src/server.rs"

ValueReply CPPIncrementer::incrementServer(ValueRequest req) {
    // lock
    this->highestReqSeen = std::max(this->highestReqSeen, req.val);
    std::cout << "highest request seen: " << this->highestReqSeen << std::endl;
    ValueReply rep;
    rep.val = req.val + 1;
    // release
    return rep;
}

int main() {
    CPPIncrementer incr;
    Args thread_args_1;
    thread_args_1.IP = "0.0.0.0:5000";
    thread_args_1.incr = &incr;
    pthread_t p1 = run_server_async(&thread_args_1);


    CPPIncrementer incr2;
    Args thread_args_2;
    thread_args_2.IP = "0.0.0.0:5002";
    thread_args_2.incr = &incr;
    pthread_t p2 = run_server_async(&thread_args_2);

    pthread_join(p1, NULL);
    pthread_join(p2, NULL);
    return 0;
}
