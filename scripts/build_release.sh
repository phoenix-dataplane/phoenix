#! /bin/bash

set -euo pipefail

main() {
    cmake -DCMAKE_BUILD_TYPE=RelWithDebInfo -Bbuild
    cmake --build build
}

main
