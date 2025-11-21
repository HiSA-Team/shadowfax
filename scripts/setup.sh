#!/bin/sh
# This script:
# - installs build dependencies for common distributions;
# - installs rust toolchain with riscv target;
#
# Author:  Giuseppe Capasso <capassog97@gmail.com>

set -e

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

if [ "$(id -u)" -ne 0 ]; then
  print_err "this script requires root privileges"
  exit 1
fi
if [ ! $SUDO_USER ]; then
  print_warn "running this script directly as root may not be what you want. Unless you know what you are doing, use sudo."
  USER_NAME="root"
  USER_HOME="/root"
else
  USER_NAME="$SUDO_USER"
  USER_HOME=$(eval echo ~$USER_NAME)
fi

# Function to determine the distribution codename from /etc/os-release
get_distro_codename() {
  local codename
  codename=$(awk -F= '/^VERSION_CODENAME=/{print $2}' /etc/os-release)
  if [ -z "$codename" ]; then
    codename=$(awk -F= '/^ID=/{print $2}' /etc/os-release)
  fi
  echo "$codename" | xargs
}

# Function to install necessary build dependencies based on the distribution codename
install_dependencies() {
  case "$DISTRO_CODENAME" in
  # ubuntu 24.04, ubuntu 22.04, debian 12
  noble | jammy | bookworm)
    apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install --no-install-recommends \
      libssl-dev qemu-system-riscv64 curl build-essential make ca-certificates git libclang-dev
    ;;
  void)
    xbps-install -Sy make base-devel openssl-devel curl qemu-system-riscv64 ca-certificates git
    ;;
  *)
    print_err "unsupported distribution: $DISTRO_CODENAME. Make sure you install dependencies according to your distribution"
    exit 1
    ;;
  esac
}

# Function to install the Rust toolchain and add the RISC-V target if not on RISC-V architecture
install_rust() {
  if ! command -v rustup &> /dev/null; then
    su $USER_NAME -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
    su $USER_NAME -c "echo PATH=~/.cargo/bin:${PATH} >> ~/.bashrc"
  fi
}

# Global variables
DISTRO_CODENAME=$(get_distro_codename)

print_info "running the script as ${USER_NAME}"
print_info "base directory: ${BASEDIR}"
print_info "detected distribution dodename: ${DISTRO_CODENAME}"

install_dependencies

# install rust if not present
if ! command -v cargo &> /dev/null; then
  install_rust
fi
su $USER_NAME -c "~/.cargo/bin/rustup show"
