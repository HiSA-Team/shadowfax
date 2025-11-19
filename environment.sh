# This script is meant to be sourced by the user to ensure correct settings are applied to
# the current shell. Based on platform (architecture and libc), it sets up the CROSS_COMPILE
# variable and LIBCLANG info. Usage:
#
# source <file>
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
BOOT_DOMAIN_ADDRESS="0x82800000"

get_libc() {
  if ldd --version 2>&1 | grep -q musl; then
    echo "musl"
  else
    echo "glibc"
  fi
}

export BOOT_DOMAIN_ADDRESS="${BOOT_DOMAIN_ADDRESS}"
print_export "BOOT_DOMAIN_ADDRESS" "${BOOT_DOMAIN_ADDRESS}"

export RUSTFLAGS="-C target-feature=+h"
print_export "RUSTFLAGS" "$RUSTFLAGS"

export LIBC=$(get_libc)
print_export "LIBC" "$LIBC"

export LIBC_PREFIX=$([ "$LIBC" = "glibc" ] && echo "gnu" || echo "$LIBC")
print_export "LIBC_PREFIX" "$LIBC_PREFIX"

if [ "$LIBC_PREFIX" = "musl" ]; then
  print_warn "Musl system detected. Make sure you provide libclang.a path in 'scripts/llvm-config.sh' accordingly and provide the path do the build directory in LIBCLANG_STATIC_PATH environment variable"

  export LIBCLANG_STATIC=1
  print_export "LIBCLANG_STATIC" "${LIBCLANG_STATIC}"

  print_export "LIBCLANG_STATIC_PATH" "${LIBCLANG_STATIC_PATH}"

  LLVM_CONFIG_PATH="$(pwd)/scripts/llvm-config.sh"
  export LLVM_CONFIG_PATH
  print_export "LLVM_CONFIG_PATH" "${LLVM_CONFIG_PATH}"
fi
