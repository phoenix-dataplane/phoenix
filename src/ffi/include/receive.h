#pragma once
#include "rust/cxx.h"
#include <iostream>

struct ValueReply;

void completeIncrement(rust::Box<ValueReply> reply);