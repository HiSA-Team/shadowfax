#!/bin/sh
# This script creates a working Linux kernel and initramfs setup.
# Output will linux-<kernel-version> folder which will contain the linux kernel build
# and the initramfs.cpio.gz.
# Args:
#   - kernel-version (mandatory): kernel version to build; i.e 6.13.2
#   - busybox-version (optional): busybox version to build. Default 1.36.1
#   - cross-compile (optional): cross-compile prefix to build on riscv if non native. Default ""
# Usage:
#   - ./setup_linux.sh --kernel <kernel-version> [--busybox <busybox>] [--cross-compile <riscv64-toolchain>]
# A quick test can be executed with:
# qemu-system-riscv64 -M virtual -m 64M \
#   -kernel linux-<kernel-version>/arch/riscv/boot/Image \
#   -initrd linux-<kernel-version>/initramfs.cpio.gz \
#   -nographic \
#   -append "console=ttyS0 root=/dev/sda earlyprintk=serial net.ifnames=0 nokaslr"
# Author:  Giuseppe Capasso <capassog97@gmail.com>

set -ev
ARCH="riscv"
KERNEL_VERSION=""
BUSYBOX_VERSION="1.36.1"
CROSS_COMPILE=""
TEMP_DIR=$(mktemp -d)

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
    --cross-compile)
      CROSS_COMPILE="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

if [ -z "$KERNEL_VERSION" ] ; then
  echo "Usage: $0 --kernel <kernel-version> [--busybox <busybox-version>] [--cross-compile <cross-compile-prefix>]"
  exit 1
fi

ODIR=$(pwd)/linux-${KERNEL_VERSION}
mkdir -p $ODIR

echo "Building kernel v${KERNEL_VERSION} with busybox v${BUSYBOX_VERSION}"

MAJOR=$(echo ${KERNEL_VERSION} | awk -F . '{print $1}')

# Get linux source code
curl -fsSL https://cdn.kernel.org/pub/linux/kernel/v${MAJOR}.x/linux-${KERNEL_VERSION}.tar.xz -o ${TEMP_DIR}/linux-${KERNEL_VERSION}.tar.xz
tar -xvf ${TEMP_DIR}/linux-${KERNEL_VERSION}.tar.xz -C ${TEMP_DIR}

# Build linux
make -C ${TEMP_DIR}/linux-${KERNEL_VERSION} ARCH=$ARCH O=${ODIR} CROSS_COMPILE=${CROSS_COMPILE} defconfig
make -C ${TEMP_DIR}/linux-${KERNEL_VERSION} ARCH=$ARCH O=${ODIR} CROSS_COMPILE=${CROSS_COMPILE} -j $(nproc) Image

# Build busybox
curl -fsSL https://busybox.net/downloads/busybox-${BUSYBOX_VERSION}.tar.bz2 -o ${TEMP_DIR}/busybox-${BUSYBOX_VERSION}.tar.bz2
tar -xvf ${TEMP_DIR}/busybox-${BUSYBOX_VERSION}.tar.bz2 -C ${TEMP_DIR}
make -C ${TEMP_DIR}/busybox-${BUSYBOX_VERSION} ARCH=$ARCH CROSS_COMPILE=${CROSS_COMPILE} defconfig

# Prepare initramfs
mkdir -p ${TEMP_DIR}/initramfs/bin
mkdir -p ${TEMP_DIR}/initramfs/etc/init.d
mkdir -p ${TEMP_DIR}/initramfs/usr

cp -r scripts/initramfs/etc/* ${TEMP_DIR}/initramfs/etc/
LDFLAGS="--static" make -C ${TEMP_DIR}/busybox-${BUSYBOX_VERSION} CONFIG_PREFIX=${TEMP_DIR}/initramfs ARCH=$ARCH CROSS_COMPILE=${CROSS_COMPILE} -j $(nproc) install
mv ${TEMP_DIR}/initramfs/linuxrc ${TEMP_DIR}/initramfs/init

find ${TEMP_DIR}/initramfs -print0 | cpio --null -ov --format=newc > ${TEMP_DIR}/initramfs.cpio
gzip ${TEMP_DIR}/initramfs.cpio
cp ${TEMP_DIR}/initramfs.cpio.gz ${ODIR}/initramfs.cpio.gz
