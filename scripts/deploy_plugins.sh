#!/usr/bin/env bash
PHOENIX_PREFIX="/tmp/phoenix"
if [[ $# -ge 1 ]]; then
    PHOENIX_PREFIX=$1
fi

mkdir -p "${PHOENIX_PREFIX}/plugins"

WORKDIR=`dirname $(realpath $0)`
TARGETDIR="${WORKDIR}"/../target/phoenix

if [[ $# -ge 2 ]]; then
    TARGETDIR=$2
fi

for plugin in `find "${TARGETDIR}"/release/ -maxdepth 1 -type f -name "libphoenix_*.rlib" -o -name "libphoenix_*.d"`; do
    install -v -Dm755 "${plugin}" -t "${PHOENIX_PREFIX}"/plugins/
done
