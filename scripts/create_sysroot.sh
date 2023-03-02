#!/usr/bin/env bash

WORKDIR=`dirname $(realpath $0)`
TARGETDIR="${WORKDIR}"/../target
SYSROOT_DIR="${TARGETDIR}"/sysroot


cargo clean
cargo build -r -p phoenix_common

cp -r "${TARGETDIR}/release/deps" "${SYSROOT_DIR}"

TOOLCHAIN_SYSROOT=`rustc --print sysroot`

cp -r "${TOOLCHAIN_SYSROOT}/lib" "${SYSROOT_DIR}"
