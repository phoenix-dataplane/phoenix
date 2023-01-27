#include "ffi/src/main.rs"
#include "../include/increment.h"

int main() {
    run("0.0.0.0:5000");
}

ValueReply incrementServer(ValueRequest req) {
    ValueReply rep;
    rep.val = req.val + 1;
    return rep;
}