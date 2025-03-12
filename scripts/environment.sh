#!/bin/sh
# This script is meant to be sourced by the user to ensure correct settings are applied to
# the current shell. Based on platform (architecture and libc), it sets up the CROSS_COMPILE
# variable.
#
# Author: Giuseppe Capasso <capassog97@gmail.com>

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
fi
