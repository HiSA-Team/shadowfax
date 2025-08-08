#!/bin/sh
# This scripts:
# - installs build dependencies for common distributions;
# - installs rust toolchain with riscv target;
# - builds and install opensbi libraries and header files;
# - builds a custom clang with static linking from llvm (only for musl systems)
#
# Author:  Giuseppe Capasso <capassog97@gmail.com>

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

if [ -z "$OPENSBI_PATH" ]; then
  echo "you may forgot to source the scripts/environment.sh file"
  exit 1
fi

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
  # ubuntu 24.04, ubuntu 22.04, debian 12, debian 11
  noble | jammy | bookworm | bullseye)
    apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install --no-install-recommends \
      qemu-system-riscv64 gcc-riscv64-linux-gnu build-essential qemu-utils \
      libncurses-dev bison flex libssl-dev device-tree-compiler python3 \
      libelf-dev dwarves curl git file cpio sudo bc libclang-dev ca-certificates
    if [ "$architecture" != "riscv64" ]; then
      DEBIAN_FRONTEND=noninteractive apt-get -y install gcc-riscv64-linux-"$LIBC_PREFIX"
    fi
    ;;
  void)
    xbps-install -Sy qemu make base-devel bison flex openssl-devel libelf \
      elfutils-devel libdwarf-devel curl git file cpio clang cmake ninja python3
    if [ "$architecture" != "riscv64" ]; then
      xbps-install -Sy cross-riscv64-linux-"$LIBC_PREFIX"
    fi
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
  su $USER_NAME -c "~/.cargo/bin/rustup show"
  su $USER_NAME -c "~/.cargo/bin/rustup component add rust-analyzer clippy"
  if [ "$ARCHITECTURE" != "riscv64" ]; then
    su $USER_NAME -c "~/.cargo/bin/rustup target add riscv64imac-unknown-none-elf"
  else
    print_info "Running on RISC-V architecture, skipping Rust RISC-V target setup."
  fi
}

# Function to download, build, and install OpenSBI
install_opensbi() {
  print_info "Downloading opensbi source"
  su $USER_NAME -c "curl -fsSL https://github.com/riscv-software-src/opensbi/archive/refs/tags/v${OPENSBI_VERSION}.tar.gz -o ${TEMP_DIR}/opensbi-${OPENSBI_VERSION}.tar.gz"

  print_info "Extracting opensbi source"
  mkdir -p ${OPENSBI_PATH}
  su $USER_NAME -c "tar xf ${TEMP_DIR}/opensbi-${OPENSBI_VERSION}.tar.gz -C ${OPENSBI_PATH} --strip-components=1"

  # build opensbi
  su $USER_NAME -c "make -C ${OPENSBI_PATH} PLATFORM=${PLATFORM}"
}

# Function to download, build, and install Clang from source for musl-based systems
build_clang_from_source() {
  print_info "Downloading LLVM source..."
  su $USER_NAME -c "curl -fsSL https://github.com/llvm/llvm-project/releases/download/llvmorg-${LLVM_VERSION}/llvm-project-${LLVM_VERSION}.src.tar.xz \
    -o ${TEMP_DIR}/llvm-project-${LLVM_VERSION}.src.tar.xz"

  print_info "Extracting LLVM source..."
  su $USER_NAME -c "tar -xf ${TEMP_DIR}/llvm-project-${LLVM_VERSION}.src.tar.xz"

  print_info "Creating build directory..."
  su $USER_NAME -c "mkdir llvm-project-${LLVM_VERSION}.src/build"

  print_info "Configuring LLVM build with CMake..."
  su $USER_NAME -c "cmake -G 'Ninja' \
    -S llvm-project-${LLVM_VERSION}.src/llvm/ \
    -B llvm-project-${LLVM_VERSION}.src/build \
    -DLLVM_ENABLE_PROJECTS='clang' \
    -DCMAKE_BUILD_TYPE=Release \
    -DLIBCLANG_BUILD_STATIC=ON \
    -DLLVM_ENABLE_ZSTD=OFF \
    -DLLVM_TARGETS_TO_BUILD='X86;RISCV' \
    -DLLVM_HOST_TRIPLE=${ARCHITECTURE}-unknown-linux-${LIBC_PREFIX}"

  print_info "Building LLVM with Ninja..."
  su $USER_NAME -c "ninja -C llvm-project-${LLVM_VERSION}.src/build"
}

# Global variables
DISTRO_CODENAME=$(get_distro_codename)
TEMP_DIR=$(mktemp -d)

# Make temp directory owned by the user
chown -R ${USER_NAME} ${TEMP_DIR}

print_info "running the script as ${USER_NAME}"
print_info "base directory: ${BASEDIR}"
print_info "detected architecture: ${ARCHITECTURE}"
print_info "detected libc: ${LIBC}"
print_info "detected distribution dodename: ${DISTRO_CODENAME}"

install_dependencies
install_rust

if [ ! -d "$OPENSBI_PATH" ]; then
  print_info "installing opensbi in ${OPENSBI_PATH}"
  install_opensbi
else
  print_warn "skipping OpenSBI installation"
fi

if [ "$LIBC_PREFIX" = "musl" ]; then
  print_warn "Building Clang from source for musl-based system. This may take some time..."
  build_clang_from_source
fi

print_info "Removing ${TEMP_DIR}..."
rm -rf ${TEMP_DIR}
