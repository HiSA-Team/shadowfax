#!/bin/sh
# This file installs dependencies for Linux systems. Distribution name is retrieved using lsb_release.
# Users must make sure lsb_release is installed on their system before running this script.
# The script installs:
# - linux kernel build dependencies
# - riscv-gnu toolchain
# - curl and other shell utilities
# - rust and rv64gc target
#
# Author:  Giuseppe Capasso <capassog97@gmail.com>

if [ "$(id -u)" -ne 0 ] || [ ! $SUDO_USER ]; then
  echo "This script must be run as root with sudo, not directly as root" >&2
  exit 1
fi


get_distro_codename() {
  local codename
  codename=$(awk -F= '/^VERSION_CODENAME=/{print $2}' /etc/os-release)
  if [ -z "$codename" ]; then
    codename=$(awk -F= '/^ID=/{print $2}' /etc/os-release)
  fi
  echo "$codename" | xargs
}

get_libc() {
  if ldd --version 2>&1 | grep -q musl; then
    echo "musl"
  else
    echo "glibc"
  fi
}

install_dependencies() {
  case "$DISTRO_CODENAME" in
    noble | jammy | bookworm | bullseye)
      apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install make qemu-system build-essential \
        libncurses-dev bison flex libssl-dev libelf-dev dwarves
      if [ "$ARCHITECTURE" != "riscv64" ]; then
        DEBIAN_FRONTEND=noninteractive apt-get -y install gcc-riscv64-linux-$LIBC_PREFIX
      fi
      ;;
    void)
      xbps-install -Sy qemu make base-devel bison flex openssl-devel libelf elfutils-devel libdwarf-devel
      if [ "$ARCHITECTURE" != "riscv64" ]; then
        xbps-install -Sy cross-riscv64-linux-$LIBC_PREFIX
      fi
      ;;
    *)
      echo "Unsupported distribution: $DISTRO_CODENAME" >&2
      echo "Make sure you install dependencies according to your distribution."
      exit 1
      ;;
  esac
}

install_rust() {
  su $USER_NAME -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
  su $USER_NAME -c "echo PATH=~/.cargo/bin:${PATH} >> ~/.bashrc"
  if [ "$ARCHITECTURE" != "riscv64" ]; then
    su $USER_NAME -c "~/.cargo/bin/rustup target add riscv64gc-unknown-none-elf"
  else
    echo "Running on RISC-V architecture, skipping Rust RISC-V target setup."
  fi
}

# Global variables
DISTRO_CODENAME=$(get_distro_codename)
USER_NAME="$SUDO_USER"
USER_HOME=$(eval echo ~$USER_NAME)
ARCHITECTURE=$(uname -m)
LIBC=$(get_libc)
LIBC_PREFIX=$([ "$LIBC" = "glibc" ] && echo "gnu" || echo "$LIBC")

echo "Running the script as $USER_NAME"
echo "Detected Architecture: $ARCHITECTURE"
echo "Detected LIBC: $LIBC"
echo "Detected Distribution Codename: $DISTRO_CODENAME"

install_dependencies
install_rust
