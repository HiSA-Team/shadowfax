#!/bin/sh
# This script creates a working Linux kernel and initramfs setup.
# Output will linux-<kernel-version> folder which will contain the linux kernel build
# and the initramfs.cpio.gz.
# Args:
#   - kernel-version (mandatory): kernel version to build; i.e 6.13.2
# Usage:
#   - ./setup_linux.sh --kernel <kernel-version> [--busybox <busybox>]
# A quick test can be executed with:
# qemu-system-riscv64 -M virtual -m 64M \
#   -kernel linux-<kernel-version>/arch/riscv/boot/Image \
#   -initrd linux-<kernel-version>/initramfs.cpio.gz \
#   -nographic \
#   -append "console=ttyS0 root=/dev/sda earlyprintk=serial net.ifnames=0 nokaslr"
#
# Author:  Giuseppe Capasso <capassog97@gmail.com>

set -e
BASEDIR=$(dirname $(realpath $0))
BUSYBOX_VERSION="1.36.1"
KERNEL_VERSION=""
TEMP_DIR=$(mktemp -d)

# use environment.sh variables
. ${BASEDIR}/environment.sh

# parse args
# Author:  Giuseppe Capasso <capassog97@gmail.com>

set -e
ARCH="riscv"
BASEDIR=$(dirname $(realpath $0))
BUSYBOX_VERSION="1.36.1"
KERNEL_VERSION=""
TEMP_DIR=$(mktemp -d)

# use environment.sh variables
. ${BASEDIR}/environment.sh

# parse args
while [ $# -gt 0 ]; do
  case $1 in
    --kernel)
      KERNEL_VERSION="$2"
      shift 2
      ;;
    --busybox)
      BUSYBOX_VERSION="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

if [ -z "$KERNEL_VERSION" ] ; then
  echo "Usage: $0 --kernel <kernel-version> [--busybox <busybox-version>]"
  exit 1
fi

ODIR=$(pwd)/linux-${KERNEL_VERSION}
mkdir -p $ODIR

echo "Building kernel v${KERNEL_VERSION} with busybox v${BUSYBOX_VERSION}"

# This function downloads and builds the linux kernel.
# Output files are created in the current directory in linux-<version>/
build_kernel() {
  MAJOR=$(echo ${KERNEL_VERSION} | awk -F . '{print $1}')

  # Get linux source code
  printf "Downloading kernel source... "
  curl -fsSL https://cdn.kernel.org/pub/linux/kernel/v${MAJOR}.x/linux-${KERNEL_VERSION}.tar.xz -o ${TEMP_DIR}/linux-${KERNEL_VERSION}.tar.xz
  printf "done\n"

  printf "Extracting kernel source... "
  tar -xf ${TEMP_DIR}/linux-${KERNEL_VERSION}.tar.xz -C ${TEMP_DIR}
  printf "done\n"

  # Build linux
  make -C ${TEMP_DIR}/linux-${KERNEL_VERSION} O=${ODIR} defconfig
  make -C ${TEMP_DIR}/linux-${KERNEL_VERSION} O=${ODIR} -j $(nproc) Image
}

# This function builds the initramfs which runs on top of the kernel providing a minimal shell environment.
# This projects uses busybox which downloaded and built as a static binary
build_initramfs() {
  # Build busybox
  printf "Downloading busybox source..."
  curl -fsSL https://busybox.net/downloads/busybox-${BUSYBOX_VERSION}.tar.bz2 -o ${TEMP_DIR}/busybox-${BUSYBOX_VERSION}.tar.bz2
  printf "done\n"

  printf "Extracting busybox source..."
  tar -xf ${TEMP_DIR}/busybox-${BUSYBOX_VERSION}.tar.bz2 -C ${TEMP_DIR}
  printf "done\n"

  make -C ${TEMP_DIR}/busybox-${BUSYBOX_VERSION} ARCH=${ARCH} defconfig

  # Prepare initramfs
  mkdir -p ${TEMP_DIR}/initramfs/bin
  mkdir -p ${TEMP_DIR}/initramfs/etc/init.d
  mkdir -p ${TEMP_DIR}/initramfs/usr

  cp -r scripts/initramfs/etc/* ${TEMP_DIR}/initramfs/etc/
  LDFLAGS="--static" make -C ${TEMP_DIR}/busybox-${BUSYBOX_VERSION} CONFIG_PREFIX=${TEMP_DIR}/initramfs -j $(nproc) install
  mv ${TEMP_DIR}/initramfs/linuxrc ${TEMP_DIR}/initramfs/init

  find "${TEMP_DIR}/initramfs" -mindepth 1 -printf '%P\0' | cpio --null -ov --format=newc --directory "${TEMP_DIR}/initramfs" > "${TEMP_DIR}/initramfs.cpio"
  gzip "${TEMP_DIR}/initramfs.cpio"
  cp "${TEMP_DIR}/initramfs.cpio.gz" "${ODIR}/initramfs.cpio.gz"
}

build_kernel
build_initramfs

printf "Removing ${TEMP_DIR}..."
rm -rf ${TEMP_DIR}
printf " done\n"

  # Get linux source code
  printf "Downloading kernel source... "
  curl -fsSL https://cdn.kernel.org/pub/linux/kernel/v${MAJOR}.x/linux-${KERNEL_VERSION}.tar.xz -o ${TEMP_DIR}/linux-${KERNEL_VERSION}.tar.xz
  printf "done\n"
  tar -xvf ${TEMP_DIR}/linux-${KERNEL_VERSION}.tar.xz -C ${TEMP_DIR}

  # Build linux
  make -C ${TEMP_DIR}/linux-${KERNEL_VERSION} O=${ODIR} defconfig
  make -C ${TEMP_DIR}/linux-${KERNEL_VERSION} O=${ODIR} -j $(nproc) Image
}

build_initramfs() {
  # Build busybox
  printf "Downloading busybox source..."
  curl -fsSL https://busybox.net/downloads/busybox-${BUSYBOX_VERSION}.tar.bz2 -o ${TEMP_DIR}/busybox-${BUSYBOX_VERSION}.tar.bz2
  printf "done\n"

  printf "Extracting busybox source..."
  tar -xf ${TEMP_DIR}/busybox-${BUSYBOX_VERSION}.tar.bz2 -C ${TEMP_DIR}
  printf "done\n"

  make -C ${TEMP_DIR}/busybox-${BUSYBOX_VERSION} ARCH=${ARCH} defconfig

  # Prepare initramfs
  mkdir -p ${TEMP_DIR}/initramfs/bin
  mkdir -p ${TEMP_DIR}/initramfs/etc/init.d
  mkdir -p ${TEMP_DIR}/initramfs/usr

  cp -r scripts/initramfs/etc/* ${TEMP_DIR}/initramfs/etc/
  LDFLAGS="--static" make -C ${TEMP_DIR}/busybox-${BUSYBOX_VERSION} CONFIG_PREFIX=${TEMP_DIR}/initramfs -j $(nproc) install
  mv ${TEMP_DIR}/initramfs/linuxrc ${TEMP_DIR}/initramfs/init

  find "${TEMP_DIR}/initramfs" -mindepth 1 -printf '%P\0' | cpio --null -ov --format=newc --directory "${TEMP_DIR}/initramfs" > "${TEMP_DIR}/initramfs.cpio"
  gzip "${TEMP_DIR}/initramfs.cpio"
  cp "${TEMP_DIR}/initramfs.cpio.gz" "${ODIR}/initramfs.cpio.gz"
}

cd ${TEMP_DIR}/initramfs
find . -print0 | cpio --null -ov --format=newc > ${TEMP_DIR}/initramfs.cpio
gzip ${TEMP_DIR}/initramfs.cpio
cp ${TEMP_DIR}/initramfs.cpio.gz ${ODIR}/initramfs.cpio.gz
cd -
build_kernel
build_initramfs

printf "Removing ${TEMP_DIR}..."
rm -rf ${TEMP_DIR}
printf " done\n"
