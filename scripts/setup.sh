 # This file installs dependencies for Linux systems. Distribution name is retrieved using lsb_release.
 # Users must make sure lsb_release is installed on their system before running this script.
 # Author:  Giuseppe Capasso <capassog97@gmail.com>
#!/bin/sh

if [ "$(id -u)" -ne 0 ] || [ ! $SUDO_USER ]; then
  echo "This script must be run as root with sudo, not directly as root" >&2
  exit 1
fi

USER_NAME="$SUDO_USER"
echo "Running the script as $USER_NAME"

# setup dependencies
DISTRO_CODENAME=$(lsb_release -c | awk '{print $2}')

case "$DISTRO_CODENAME" in
  noble | jammy)
    apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install make qemu-system \
      gcc-riscv64-linux-gnu build-essential libncurses-dev bison flex libssl-dev libelf-dev dwarves
    ;;
  bookworm | bullseye)
    apt update && DEBIAN_FRONTEND=noninteractive apt -y install make qemu-system \
      gcc-riscv64-linux-gnu build-essential libncurses-dev bison flex libssl-dev libelf-dev dwarves
    ;;
  void)
    xbps-install -Sy qemu make cross-riscv64-linux-gnu base-devel bison flex openssl-devel \
      libelf elfutils-devel libdwarf-devel
    ;;
  *)
    echo "Unsupported distribution: $DISTRO_CODENAME" >&2
    echo "Make sure you install dependencies according to your distribution."
    exit 1
    ;;
esac
