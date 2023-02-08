#!/usr/bin/env bash
prefix="/tmp/phoenix"
if [[ $# -ge 1 ]]; then
    prefix=$1
fi

mkdir -p $prefix/plugins

WORKDIR=`dirname $(realpath $0)`
TARGETDIR="${WORKDIR}"/../target

for plugin in `find "${TARGETDIR}"/release/ -maxdepth 1 -type f -name "libphoenix_*.rlib" -o -name "libphoenix_*.d"`; do
    install -v -Dm755 "${plugin}" -t "${prefix}"/plugins/
done
