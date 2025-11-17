#!/bin/sh
# This script is meant to be sourced by the user to ensure correct settings are applied to
# the current shell. Based on platform (architecture and libc), it sets up the CROSS_COMPILE
# variable and LIBCLANG info. Usage:
#
# source <file> <opensbi-path>
#
# Author: Giuseppe Capasso <capassog97@gmail.com>

# Colors
RED='\033[31m'
GREEN='\033[32m'
BLUE='\033[34m'
YELLOW='\033[33m'
RESET='\033[0m'

# Helpers (print to stderr so stdout remains clean for callers)
print_err() { printf '%b[ERROR]%b %s\n' "$RED" "$RESET" "$1" >&2; }
print_info() { printf '%b[INFO]%b %s\n' "$GREEN" "$RESET" "$1" >&2; }
print_export() { printf '%b[EXPORT]%b %s=%s\n' "$BLUE" "$RESET" "$1" "$2" >&2; }
print_warn() { printf '%b[WARNING]%b %s\n' "$YELLOW" "$RESET" "$1" >&2; }

# Config parameters
OPENSBI_PATH="$1"
LLVM_VERSION="${LLVM_VERSION:-17.0.6}"
OPENSBI_VERSION="${OPENSBI_VERSION:-1.6}"
PLATFORM="${PLATFORM:-generic}"

if [ -z "$OPENSBI_PATH" ]; then
  print_err "missing OPENSBI_PATH"
  return 1
fi

export OPENSBI_PATH="${OPENSBI_PATH}"
print_export "OPENSBI_PATH" "${OPENSBI_PATH}"

export OPENSBI_VERSION="${OPENSBI_VERSION}"
print_export "OPENSBI_VERSION" "${OPENSBI_VERSION}"

export PLATFORM="${PLATFORM}"
print_export "PLATFORM" "${PLATFORM}"

get_libc() {
  if ldd --version 2>&1 | grep -q musl; then
    echo "musl"
  else
    echo "glibc"
  fi
}

export ARCHITECTURE=$(uname -m)
print_export "ARCHITECTURE" "$ARCHITECTURE"

export LIBC=$(get_libc)
print_export "LIBC" "$LIBC"

export LIBC_PREFIX=$([ "$LIBC" = "glibc" ] && echo "gnu" || echo "$LIBC")
print_export "LIBC_PREFIX" "$LIBC_PREFIX"

# Export CROSS_COMPILE if not on riscv64
if [ "$ARCHITECTURE" != "riscv64" ]; then
  CROSS_COMPILE="riscv64-linux-${LIBC_PREFIX}-"
  export CROSS_COMPILE
  print_export "CROSS_COMPILE" "${CROSS_COMPILE}"

  export ARCH=riscv
  print_export "ARCH" "${ARCH}"
fi

if [ "$LIBC_PREFIX" = "musl" ]; then
  export LLVM_VERSION="${LLVM_VERSION}"
  print_export "LLVM_VERSION" "${LLVM_VERSION}"

  export LIBCLANG_STATIC=1
  print_export "LIBCLANG_STATIC" "${LIBCLANG_STATIC}"

  LIBCLANG_PATH="$(pwd)/llvm-project-${LLVM_VERSION}.src/build/lib"
  export LIBCLANG_PATH
  print_export "LIBCLANG_PATH" "${LIBCLANG_PATH}"

  LIBCLANG_STATIC_PATH="$(pwd)/llvm-project-${LLVM_VERSION}.src/build/lib"
  export LIBCLANG_STATIC_PATH
  print_export "LIBCLANG_STATIC_PATH" "${LIBCLANG_STATIC_PATH}"

  LLVM_CONFIG_PATH="$(pwd)/scripts/llvm-config.sh"
  export LLVM_CONFIG_PATH
  print_export "LLVM_CONFIG_PATH" "${LLVM_CONFIG_PATH}"
fi
