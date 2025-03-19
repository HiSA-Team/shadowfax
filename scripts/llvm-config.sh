#!/bin/sh

BASEDIR=$(dirname $(realpath $0))
LLVM_PATH=${BASEDIR}/../../llvm-project/build/bin/llvm-config

if [ "$1" = "--libs" ]; then
    ${LLVM_PATH} "$@" "--link-static"
else
    ${LLVM_PATH} "$@"
fi
