#!/bin/sh

# source correct variables
BASEDIR=$(dirname $(realpath $0))
. ${BASEDIR}/environment.sh

LLVM_PATH=${BASEDIR}/../llvm-project-${LLVM_VERSION}.src/build/bin/llvm-config

if [ "$1" = "--libs" ]; then
    ${LLVM_PATH} "$@" "--link-static"
else
    ${LLVM_PATH} "$@"
fi
