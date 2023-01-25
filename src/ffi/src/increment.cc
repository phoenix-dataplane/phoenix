#include "../include/increment.h"

ValueReply incrementServer(ValueRequest req) {
    ValueReply rep;
    rep.val = req.val + 50;
    return rep;
}