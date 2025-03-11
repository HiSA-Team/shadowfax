#!/bin/sh
# This file installs dependencies for Linux systems. Distribution name is retrieved using lsb_release.
# Users must make sure lsb_release is installed on their system before running this script.
#
# Author:  Giuseppe Capasso <capassog97@gmail.com>

if [ "$(id -u)" -ne 0 ] || [ ! $SUDO_USER ]; then
  echo "This script must be run as root with sudo, not directly as root" >&2
  exit 1
fi

USER_NAME="$SUDO_USER"
USER_HOME=$(eval echo ~$USER_NAME)

echo "Running the script as $USER_NAME"

# setup dependencies
DISTRO_CODENAME=$(lsb_release -c | awk '{print $2}')

case "$DISTRO_CODENAME" in
  noble | jammy)
    apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install make qemu-system \
      gcc-riscv64-linux-gnu curl
    ;;
  bookworm | bullseye)
    apt update && DEBIAN_FRONTEND=noninteractive apt -y install make qemu-system \
      gcc-riscv64-linux-gnu curl
    ;;
  void)
    xbps-install -Sy qemu make cross-riscv64-linux-gnu curl
    ;;
  *)
    echo "Unsupported distribution: $DISTRO_CODENAME" >&2
    echo "Make sure you install dependencies according to your distribution."
    exit 1
    ;;
esac

# Setup rust toolchain and cross-compile
# Install rustup: the official toolchain manager
su $USER_NAME -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"

# Install riscv64 target
su $USER_NAME -c "rustup target add riscv64gc-unknown-none-elf"
