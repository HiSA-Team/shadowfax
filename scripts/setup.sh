#!/bin/sh
# This scripts:
# - installs build dependencies for common distributions;
# - installs rust toolchain with riscv target;
# - builds and install opensbi libraries and header files;
#
# Author:  Giuseppe Capasso <capassog97@gmail.com>

if [ "$(id -u)" -ne 0 ]; then
  echo "This script requires root privileges"
  exit 1
fi
if [ ! $SUDO_USER ]; then
  echo "\033[33mWARNING\033[0m: running this script directly as root may not be what you want. Unless you know what you are doing, use sudo." >&2
fi

# use environment.sh variables
BASEDIR=$(dirname $(realpath $0))
. ${BASEDIR}/environment.sh

get_distro_codename() {
  local codename
  codename=$(awk -F= '/^VERSION_CODENAME=/{print $2}' /etc/os-release)
  if [ -z "$codename" ]; then
    codename=$(awk -F= '/^ID=/{print $2}' /etc/os-release)
  fi
  echo "$codename" | xargs
}

install_dependencies() {
  case "$DISTRO_CODENAME" in
    # Ubuntu 24.04, Ubuntu 22.04, Debian 12, Debian 11
    noble | jammy | bookworm | bullseye)
      apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install make qemu-system build-essential \
        libncurses-dev bison flex libssl-dev libelf-dev dwarves curl git file bc cpio
      if [ "$ARCHITECTURE" != "riscv64" ]; then
        DEBIAN_FRONTEND=noninteractive apt-get -y install gcc-riscv64-linux-$LIBC_PREFIX
      fi
      ;;
    void)
      xbps-install -Sy qemu make base-devel bison flex openssl-devel libelf elfutils-devel libdwarf-devel \
        curl git file cpio
        git
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

install_opensbi() {
  printf "Downloading opensbi source..."
  su $USER_NAME -c "curl -fsSL https://github.com/riscv-software-src/opensbi/archive/refs/tags/v${OPENSBI_VERSION}.tar.gz -o ${TEMP_DIR}/opensbi-${OPENSBI_VERSION}.tar.gz"
  printf " done\n"

  su $USER_NAME -c "tar xvf ${TEMP_DIR}/opensbi-${OPENSBI_VERSION}.tar.gz -C ${TEMP_DIR}"

  # build opensbi
  su $USER_NAME -c "make -C ${TEMP_DIR}/opensbi-${OPENSBI_VERSION} PLATFORM=${PLATFORM}"

  # install opensbi in root directory
  su $USER_NAME -c "make -C ${TEMP_DIR}/opensbi-${OPENSBI_VERSION} I=${BASEDIR}/.. install"
}

# Global variables
DISTRO_CODENAME=$(get_distro_codename)
OPENSBI_VERSION="${OPENSBI_VERSION:-1.6}"
PLATFORM="${PLATFORM:-generic}"
TEMP_DIR=$(mktemp -d)
USER_NAME="${SUDO_USER:-root}"

# Make temp directory owned by the user
chown -R ${USER_NAME} ${TEMP_DIR}

echo "Running the script as ${USER_NAME}"
echo "Base directory: ${BASEDIR}"
echo "Detected Architecture: ${ARCHITECTURE}"
echo "Detected LIBC: ${LIBC}"
echo "Detected Distribution Codename: ${DISTRO_CODENAME}"

install_dependencies
install_rust
install_opensbi

printf "Removing ${TEMP_DIR}..."
rm -rf ${TEMP_DIR}
printf " done\n"
