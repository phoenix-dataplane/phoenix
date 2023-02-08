#include "ffi/src/server.rs"
#include "../include/increment.h"
#include <iostream>

ValueReply CPPIncrementer::incrementServer(ValueRequest req) {
    // lock
    this->highestReqSeen = this->highestReqSeen > req.val? this->highestReqSeen : req.val;
    std::cout << "highest request seen: " << this->highestReqSeen << std::endl;
    ValueReply rep;
    rep.val = req.val + 1;
    // release
    return rep;
}

int main() {
    CPPIncrementer incr;
    incr.highestReqSeen = 0;

    // register a service to a socket.
    run("0.0.0.0:5000", incr);
}

