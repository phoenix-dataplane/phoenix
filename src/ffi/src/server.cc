#include "ffi/src/server.rs"
#include "../include/increment.h"

ValueReply incrementServer(ValueRequest req) {
    this.highestReqSeen++;
    ValueReply rep;
    rep.val = req.val + 1;
    return rep;
}

int main() {
    CPPIncrementer incr;
    incr.highestReqSeen = 0;

    // register a service to a socket.
    run("0.0.0.0:5000", incr);
}

