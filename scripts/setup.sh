#!/bin/sh
# This scripts:
# - installs build dependencies for common distributions;
# - installs rust toolchain with riscv target;
# - builds and install opensbi libraries and header files;
# - builds a custom clang with static linking from llvm (only for musl systems)
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
  case "$distro_codename" in
  # ubuntu 24.04, ubuntu 22.04, debian 12, debian 11
  noble | jammy | bookworm | bullseye)
    apt-get update && debian_frontend=noninteractive apt-get -y install \
      make qemu-system build-essential libncurses-dev bison flex libssl-dev \
      libelf-dev dwarves curl git file bc cpio clang cmake ninja-build
    if [ "$architecture" != "riscv64" ]; then
      debian_frontend=noninteractive apt-get -y install gcc-riscv64-linux-"$libc_prefix"
    fi
    ;;
  void)
    xbps-install -sy qemu make base-devel bison flex openssl-devel libelf \
      elfutils-devel libdwarf-devel curl git file cpio clang cmake ninja
    if [ "$architecture" != "riscv64" ]; then
      xbps-install -sy cross-riscv64-linux-"$libc_prefix"
    fi
    ;;
  *)
    echo "unsupported distribution: $distro_codename" >&2
    echo "make sure you install dependencies according to your distribution."
    exit 1
    ;;
  esac
}

# Function to install the Rust toolchain and add the RISC-V target if not on RISC-V architecture
install_rust() {
  su $USER_NAME -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
  su $USER_NAME -c "echo PATH=~/.cargo/bin:${PATH} >> ~/.bashrc"
  if [ "$ARCHITECTURE" != "riscv64" ]; then
    su $USER_NAME -c "~/.cargo/bin/rustup target add riscv64gc-unknown-none-elf"
  else
    echo "Running on RISC-V architecture, skipping Rust RISC-V target setup."
  fi
}

# Function to download, build, and install OpenSBI
install_opensbi() {
  printf "Downloading opensbi source..."
  su $USER_NAME -c "curl -fsSL https://github.com/riscv-software-src/opensbi/archive/refs/tags/v${OPENSBI_VERSION}.tar.gz -o ${TEMP_DIR}/opensbi-${OPENSBI_VERSION}.tar.gz"
  printf " done\n"

  printf "Extracting opensbi source..."
  su $USER_NAME -c "tar xf ${TEMP_DIR}/opensbi-${OPENSBI_VERSION}.tar.gz -C ${TEMP_DIR}"
  printf " done\n"

  # build opensbi
  su $USER_NAME -c "make -C ${TEMP_DIR}/opensbi-${OPENSBI_VERSION} PLATFORM=${PLATFORM}"

  # install opensbi in root directory
  su $USER_NAME -c "make -C ${TEMP_DIR}/opensbi-${OPENSBI_VERSION} I=${BASEDIR}/.. PLATFORM=${PLATFORM} install"
}

# Function to download, build, and install Clang from source for musl-based systems
build_clang_from_source() {
  printf "Downloading LLVM source..."
  su $USER_NAME -c "curl -fsSL https://github.com/llvm/llvm-project/releases/download/llvmorg-${LLVM_VERSION}/llvm-project-${LLVM_VERSION}.src.tar.xz \
    -o ${TEMP_DIR}/llvm-project-${LLVM_VERSION}.src.tar.xz"
  printf " done\n"

  printf "Extracting LLVM source..."
  su $USER_NAME -c "tar -xf ${TEMP_DIR}/llvm-project-${LLVM_VERSION}.src.tar.xz"
  printf " done\n"

  printf "Creating build directory..."
  su $USER_NAME -c "mkdir llvm-project-${LLVM_VERSION}.src/build"
  printf " done\n"

  printf "Configuring LLVM build with CMake..."
  su $USER_NAME -c "cmake -G 'Ninja' \
    -S llvm-project-${LLVM_VERSION}.src/llvm/ \
    -B llvm-project-${LLVM_VERSION}.src/build \
    -DLLVM_ENABLE_PROJECTS='clang' \
    -DCMAKE_BUILD_TYPE=Release \
    -DLIBCLANG_BUILD_STATIC=ON \
    -DLLVM_ENABLE_ZSTD=OFF \
    -DLLVM_TARGETS_TO_BUILD='X86;RISCV' \
    -DLLVM_HOST_TRIPLE=${ARCHITECTURE}-unknown-linux-${LIBC_PREFIX}"
  printf " done\n"

  printf "Building LLVM with Ninja...\n"
  su $USER_NAME -c "ninja -C llvm-project-${LLVM_VERSION}.src/build"
  printf " done\n"
}

# Global variables
DISTRO_CODENAME=$(get_distro_codename)
TEMP_DIR=$(mktemp -d)
USER_NAME="${SUDO_USER:-root}"
install_opensbi() {
  printf "Downloading opensbi source..."
  su $USER_NAME -c "curl -fsSL https://github.com/riscv-software-src/opensbi/archive/refs/tags/v${OPENSBI_VERSION}.tar.gz -o ${TEMP_DIR}/opensbi-${OPENSBI_VERSION}.tar.gz"
  printf " done\n"

  su $USER_NAME -c "tar xvf ${TEMP_DIR}/opensbi-${OPENSBI_VERSION}.tar.gz -C ${TEMP_DIR}"

  # build opensbi
  su $USER_NAME -c "make -C ${TEMP_DIR}/opensbi-${OPENSBI_VERSION} PLATFORM=${PLATFORM}"

  # install opensbi in root directory
  su $USER_NAME -c "make -C ${TEMP_DIR}/opensbi-${OPENSBI_VERSION} I=${BASEDIR}/.. PLATFORM=${PLATFORM} install"
}

# Global variables
DISTRO_CODENAME=$(get_distro_codename)
OPENSBI_VERSION="${OPENSBI_VERSION:-1.6}"
PLATFORM="${PLATFORM:-generic}"
TEMP_DIR=$(mktemp -d)
USER_NAME="$SUDO_USER"
USER_HOME=$(eval echo ~$USER_NAME)

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

if [ "$LIBC_PREFIX" = "musl" ]; then
  echo "Building Clang from source for musl-based system..."
  build_clang_from_source
fi

# use environment.sh variables
. ${BASEDIR}/environment.sh

install_opensbi

printf "Removing ${TEMP_DIR}..."
rm -rf ${TEMP_DIR}
printf " done\n"
