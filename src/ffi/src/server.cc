#include "ffi/src/server.rs"
#include "../include/increment.h"

ValueReply incrementServer(ValueRequest req) {
    ValueReply rep;
    rep.val = req.val + 1;
    return rep;
}

int main() {
    CPPIncrementer incr;
    incr.highestReqSeen = 0;
    run("0.0.0.0:5000", incr);
}

