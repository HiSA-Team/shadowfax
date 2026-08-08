# Shadowfax Setup

This guide contains the detailed host, toolchain, key, and Linux preparation steps. For the shortest
runnable path, start with the [README](README.md).

## Clone the repository

OpenSBI and several workloads are Git submodules:

```sh
git clone --recurse-submodules https://github.com/HiSA-Team/shadowfax
cd shadowfax
```

For an existing clone, run `git submodule update --init --recursive`.

## Host dependencies

Ubuntu 22.04/24.04 and Debian 12 users can install the base dependencies with:

```sh
sudo ./scripts/setup.sh
```

Other systems need equivalent packages for OpenSSL development files, libclang, Make, Git, QEMU
RISC-V, a device-tree compiler (`dtc` and `fdtput`), and standard C build tools. The Linux launcher
also expects `od`, `stat`, and Bash.

The pinned Rust nightly and bare-metal target are declared in `rust-toolchain.toml`; `rustup` selects
them automatically inside the repository.

## RISC-V toolchains

The build requires a 64-bit RISC-V GCC/binutils toolchain. Check what the Makefile detects:

```sh
make build-info
```

If necessary, add the toolchain's `bin/` directory to `PATH` or override the prefix:

```sh
make RV_PREFIX=/opt/riscv/bin/riscv64-unknown-elf- build-info
```

The standalone launcher defaults to `riscv64-unknown-elf-`; Linux builds commonly use
`riscv64-unknown-linux-gnu-`. Keep the selected compiler's ISA and ABI compatible with RV64.

## Python and attestation tooling

Measured firmware builds use `scripts/dice_tool.py`, which requires `cbor2`. Either install it in a
virtual environment or use an ephemeral `uv` environment:

```sh
python -m venv .venv
source .venv/bin/activate
pip install cbor2
```

```sh
make -B PYTHON='uv run --with cbor2'
```

The `-B` option is important after firmware or measurement changes because it regenerates signatures
and DICE attestation input even when timestamps would otherwise reuse an artifact.

## Keys and firmware build

Generate local ED25519 signing keys and DICE root-of-trust keys once:

```sh
make generate-keys PYTHON='uv run --with cbor2'
```

Then build all guests, the TSM, firmware, signatures, and attestation data:

```sh
make -B PYTHON='uv run --with cbor2'
```

Generated keys are development material under `shadowfax/keys/`; do not treat them as production
secrets or commit private replacements.

## Linux host

The Linux tree and build output are intentionally local. `scripts/run-linux.sh` expects, by default:

```text
linux/host/arch/riscv/boot/Image
bin/initramfs.cpio.gz
```

### Kernel configuration

Start with a small RV64 configuration and enable at least:

```text
CONFIG_64BIT=y
CONFIG_MMU=y
CONFIG_BLK_DEV_INITRD=y
CONFIG_RD_GZIP=y
CONFIG_SERIAL_8250=y
CONFIG_SERIAL_8250_CONSOLE=y
CONFIG_DEVTMPFS=y
CONFIG_DEVTMPFS_MOUNT=y
CONFIG_NET=y
CONFIG_INET=y
CONFIG_IP_PNP=y
CONFIG_IP_PNP_DHCP=y
CONFIG_NETDEVICES=y
CONFIG_VIRTIO=y
CONFIG_VIRTIO_MMIO=y
CONFIG_VIRTIO_NET=y
CONFIG_UNIX=y
CONFIG_UNIX98_PTYS=y
```

One out-of-tree build pattern is:

```sh
LINUX_SRC=/path/to/linux
mkdir -p linux/host
export KCONFIG_CONFIG="$PWD/linux/host/kconfig"
make -C "$LINUX_SRC" O="$PWD/linux/host" ARCH=riscv \
    CROSS_COMPILE=riscv64-unknown-linux-gnu- olddefconfig
make -C "$LINUX_SRC" O="$PWD/linux/host" ARCH=riscv \
    CROSS_COMPILE=riscv64-unknown-linux-gnu- -j"$(nproc)" Image
```

### Minimal initramfs

The archive may be an uncompressed `.cpio` or gzip-compressed `.cpio.gz`. It needs BusyBox or an
equivalent `/init`. For an interactive shell and Dropbear, initialize the virtual filesystems before
starting SSH:

```sh
mkdir -p /proc /sys /dev /dev/pts /tmp /run /etc/dropbear
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
mkdir -p /dev/pts
mount -t devpts devpts /dev/pts
mount -t tmpfs tmpfs /tmp
dropbear -R -p 0.0.0.0:22
```

For public-key login, provide a root entry in `/etc/passwd`, `/etc/group`, and the host public key in
`/root/.ssh/authorized_keys`. Embed a Dropbear host key under `/etc/dropbear/` if stable host identity
is required across boots.

### Launch

Run the default artifacts with:

```sh
./scripts/run-linux.sh
```

The script creates a private DTB containing the exact initramfs range, checks every loaded range for
overlap, and forwards `127.0.0.1:2222` to guest port 22. QEMU's DHCP server configures the guest; no
fixed guest IP is required.

Override inputs without editing the script:

```sh
LINUX_IMAGE=/path/to/Image \
INITRAMFS=/path/to/initramfs.cpio.gz \
SSH_FORWARD_PORT=2223 \
./scripts/run-linux.sh
```

## Docker

The provided image builds a controlled development environment, including QEMU and a RISC-V
toolchain:

```sh
docker build -t shadowfax-build --build-arg USER_ID="$(id -u)" .
docker run --rm -it -v "$PWD:/shadowfax" -w /shadowfax \
    shadowfax-build sh -c 'make build-info'
```

The repository also includes a devcontainer configuration for compatible editors.

## musl hosts

On musl hosts, `clang-sys` requires static libclang artifacts. Build LLVM/Clang with
`LIBCLANG_BUILD_STATIC=ON`, then export `LIBCLANG_STATIC_PATH` to the resulting library directory.
The root Makefile uses `scripts/llvm-config.sh` as `LLVM_CONFIG_PATH` for this configuration.

For example:

```sh
git clone https://github.com/llvm/llvm-project.git
cmake -S llvm-project/llvm -B llvm-project/build -G Ninja \
    -DLLVM_ENABLE_PROJECTS=clang \
    -DLIBCLANG_BUILD_STATIC=ON
ninja -C llvm-project/build
export LIBCLANG_STATIC_PATH="$PWD/llvm-project/build/lib"
```

## Pre-commit checks

Install the hooks before contributing:

```sh
uv tool install pre-commit
pre-commit install
```
