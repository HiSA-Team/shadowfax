#!/bin/sh
# This script is meant to be sourced by the user to ensure correct settings are applied to
# the current shell. Based on platform (architecture and libc), it sets up the CROSS_COMPILE
# variable.
#
# Author: Giuseppe Capasso <capassog97@gmail.com>

# Config parameters
LLVM_VERSION="${LLVM_VERSION:-17.0.6}"
OPENSBI_VERSION="${OPENSBI_VERSION:-1.6}"
PLATFORM="${PLATFORM:-generic}"


get_libc() {
  if ldd --version 2>&1 | grep -q musl; then
    echo "musl"
  else
    echo "glibc"
  fi
}

ARCHITECTURE=$(uname -m)
LIBC=$(get_libc)
LIBC_PREFIX=$([ "$LIBC" = "glibc" ] && echo "gnu" || echo "$LIBC")

# Export CROSS_COMPILE if not on riscv64
if [ "$ARCHITECTURE" != "riscv64" ]; then
  export CROSS_COMPILE="riscv64-linux-$LIBC_PREFIX-"
  export ARCH=riscv
fi

if [ "$LIBC_PREFIX" = "musl" ]; then
  export LIBCLANG_STATIC=1
  export LIBCLANG_PATH=$(pwd)/llvm-project-${LLVM_VERSION}.src/build/lib
  export LIBCLANG_STATIC_PATH=$(pwd)/llvm-project-${LLVM_VERSION}.src/build/lib
  export LLVM_CONFIG_PATH=$(pwd)/scripts/llvm-config.sh
fi
