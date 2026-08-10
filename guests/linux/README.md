# Linux TVM guest

This directory describes Linux running as a confidential TVM guest in VS-mode. It is distinct from
the untrusted Linux host documented in the repository's main `SETUP.md`.

The committed inputs are:

- `kernel.config`: tested Linux 7.1 RISC-V configuration.
- `busybox.config`: tested BusyBox 1.38 configuration for the initramfs.
- `linux-tvm.dts`: minimal TVM device tree describing one CPU, 64 MiB of RAM, and the UART mapping.

Generated outputs stay outside this directory:

```text
linux/guest/vmlinux
bin/linux-tvm-initramfs.cpio.gz
bin/linux-tvm.dtb
```

## Build the guest kernel

From the repository root, copy the committed configuration into an out-of-tree Linux build:

```sh
PROJECT_ROOT="$PWD"
LINUX_SRC=/path/to/linux
LINUX_OUT="$PROJECT_ROOT/linux/guest"

mkdir -p "$LINUX_OUT"
cp guests/linux/kernel.config "$LINUX_OUT/.config"
make -C "$LINUX_SRC" O="$LINUX_OUT" ARCH=riscv \
    CROSS_COMPILE=riscv64-unknown-linux-gnu- olddefconfig
make -C "$LINUX_SRC" O="$LINUX_OUT" ARCH=riscv \
    CROSS_COMPILE=riscv64-unknown-linux-gnu- -j"$(nproc)" vmlinux
```

The configuration deliberately enables `CONFIG_FPU` because the current static userspace contains
F/D instructions from its RV64GC runtime libraries. It also enables `CONFIG_POSIX_TIMERS`; BusyBox
`ping` uses `setitimer(ITIMER_REAL)` to schedule packets after the first request.

## Build BusyBox

Use the same Linux cross-toolchain and an out-of-tree BusyBox build:

```sh
BUSYBOX_SRC=/path/to/busybox-1.38.0
BUSYBOX_OUT="$PROJECT_ROOT/build/busybox-linux-tvm"
ROOTFS="$PROJECT_ROOT/build/linux-tvm-rootfs"

mkdir -p "$BUSYBOX_OUT" "$ROOTFS"
cp guests/linux/busybox.config "$BUSYBOX_OUT/.config"
make -C "$BUSYBOX_SRC" O="$BUSYBOX_OUT" \
    CROSS_COMPILE=riscv64-unknown-linux-gnu- oldconfig
make -C "$BUSYBOX_SRC" O="$BUSYBOX_OUT" \
    CROSS_COMPILE=riscv64-unknown-linux-gnu- -j"$(nproc)"
make -C "$BUSYBOX_SRC" O="$BUSYBOX_OUT" \
    CROSS_COMPILE=riscv64-unknown-linux-gnu- CONFIG_PREFIX="$ROOTFS" install
```

Add an executable `/init` and the required `/etc` files to `ROOTFS`, then create the archive:

```sh
(cd "$ROOTFS" && find . -print0 | cpio --null -ov --format=newc | gzip -9) \
    > bin/linux-tvm-initramfs.cpio.gz
```

## Build the device tree

Compile the base tree, then patch the initramfs end address to match the generated archive. The TSM
currently places the initramfs at guest physical address `0x01000000`.

```sh
dtc -I dts -O dtb -o bin/linux-tvm.dtb guests/linux/linux-tvm.dts

INITRAMFS_START=$((0x01000000))
INITRAMFS_SIZE=$(stat -c %s bin/linux-tvm-initramfs.cpio.gz)
INITRAMFS_END=$((INITRAMFS_START + INITRAMFS_SIZE))

fdtput -t x bin/linux-tvm.dtb /chosen linux,initrd-start \
    0 "0x$(printf '%x' "$INITRAMFS_START")"
fdtput -t x bin/linux-tvm.dtb /chosen linux,initrd-end \
    0 "0x$(printf '%x' "$INITRAMFS_END")"
```

## Boot the standalone TSM

The current standalone entrypoint embeds the three artifacts above and starts the Linux TVM using
the lazy ELF loader:

```sh
cargo build --target riscv64imac-unknown-none-elf -p tsm
qemu-system-riscv64 -M virt -nographic -smp 1 -m 512M \
    -kernel target/riscv64imac-unknown-none-elf/debug/tsm
```

The tested setup provides one vCPU, guest RAM at GPA `0x00200000` with a size of 64 MiB, an initramfs
at GPA `0x01000000`, and a UART at guest GPA `0x05000000`. It is a minimal console environment and
does not currently describe a PLIC or a virtio network device.
