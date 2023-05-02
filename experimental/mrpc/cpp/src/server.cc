#include "../include/increment.h"
#include "ffi/src/servercodegen.rs"

const ValueReply& CPPIncrementer::incrementServer(const ValueRequest& req) {
    // lock
    this->highestReqSeen = std::max(this->highestReqSeen, req.val());
    std::cout << "highest request seen: " << this->highestReqSeen << std::endl;
    std::cout << "[" << unsigned(req.key(0));
    for (size_t i = 1; i < req.key_size(); i++) {
        std::cout << ", " << unsigned(req.key(i));
    }
    std::cout << "]" << std::endl;
    std::cout << "done" << std::endl;
    
    ValueReply* rep = new_value_reply().into_raw();
    
    rep->set_val(req.val() + 1);
    // release
    return *rep;
}

int main() {
    CPPIncrementer incr;
    Args thread_args_1;
    thread_args_1.IP = "0.0.0.0:5000";
    thread_args_1.incr = &incr;
    pthread_t p1 = run_async(&thread_args_1);


    CPPIncrementer incr2;
    Args thread_args_2;
    thread_args_2.IP = "0.0.0.0:5002";
    thread_args_2.incr = &incr2;
    pthread_t p2 = run_async(&thread_args_2);

    pthread_join(p1, NULL);
    pthread_join(p2, NULL);
    return 0;
}