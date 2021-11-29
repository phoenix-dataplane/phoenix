#! /bin/bash

set -euo pipefail

main() {
    cmake -DCMAKE_BUILD_TYPE=Release -Bbuild
    cmake --build build
}

main
