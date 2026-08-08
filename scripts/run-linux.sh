#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
PROJECT_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)

# Keep the toolchain setup identical to direct Makefile use.
source "$PROJECT_ROOT/config.rc"

LINUX_IMAGE_PATH=${LINUX_IMAGE:-"$PROJECT_ROOT/linux/host/arch/riscv/boot/Image"}
INITRAMFS_IMAGE_PATH=${INITRAMFS:-"$PROJECT_ROOT/bin/initramfs.cpio.gz"}
SHADOWFAX_PYTHON=${SHADOWFAX_PYTHON:-"uv run --with cbor2"}
PLATFORM_NAME=${PLATFORM:-generic}
SSH_FORWARD_PORT=${SSH_FORWARD_PORT:-2222}

LINUX_LOAD_ADDR=$((0x8a000000))
SUPERVISOR_REGION_SIZE=$((96 * 1024 * 1024))
SUPERVISOR_END_ADDR=$((LINUX_LOAD_ADDR + SUPERVISOR_REGION_SIZE))
INITRAMFS_ALIGNMENT=$((0x200000))
FDT_LOAD_ADDR=$((0x8bf00000))

usage() {
    cat <<EOF
Usage: scripts/run-linux.sh [initramfs]

Environment overrides:
  LINUX_IMAGE=/path/to/Image
  INITRAMFS=/path/to/initramfs.cpio.gz
  SSH_FORWARD_PORT=2222
  SHADOWFAX_PYTHON='uv run --with cbor2'
  QEMU_DEVICES='additional QEMU arguments'
EOF
}

die() {
    echo "run-linux.sh: $*" >&2
    exit 1
}

print_range() {
    local label=$1
    local start_addr=$2
    local end_addr=$3
    local size_bytes=$4
    local image_path=$5
    local size_kib_x100=$(((size_bytes * 100 + 512) / 1024))
    local size_mib_x100=$(((size_bytes * 100 + 524288) / 1048576))

    printf '%-13s: [0x%x, 0x%x) %d bytes (%d.%02d KiB, %d.%02d MiB) (%s)\n' \
        "$label" "$start_addr" "$end_addr" "$size_bytes" \
        "$((size_kib_x100 / 100))" "$((size_kib_x100 % 100))" \
        "$((size_mib_x100 / 100))" "$((size_mib_x100 % 100))" \
        "$image_path"
}

check_no_overlap() {
    local first_label=$1
    local first_start=$2
    local first_end=$3
    local second_label=$4
    local second_start=$5
    local second_end=$6

    if (( first_start < second_end && second_start < first_end )); then
        die "$first_label [0x$(printf '%x' "$first_start"), 0x$(printf '%x' "$first_end")) overlaps $second_label [0x$(printf '%x' "$second_start"), 0x$(printf '%x' "$second_end"))"
    fi
}

check_in_supervisor_region() {
    local label=$1
    local start_addr=$2
    local end_addr=$3

    if (( start_addr < LINUX_LOAD_ADDR || SUPERVISOR_END_ADDR < end_addr )); then
        die "$label [0x$(printf '%x' "$start_addr"), 0x$(printf '%x' "$end_addr")) is outside supervisor RAM [0x$(printf '%x' "$LINUX_LOAD_ADDR"), 0x$(printf '%x' "$SUPERVISOR_END_ADDR"))"
    fi
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

if (( $# > 1 )); then
    usage >&2
    exit 2
fi

if (( $# == 1 )); then
    INITRAMFS_IMAGE_PATH=$1
fi

[[ -f "$LINUX_IMAGE_PATH" ]] || die "Linux image not found: $LINUX_IMAGE_PATH"
[[ -f "$INITRAMFS_IMAGE_PATH" ]] || die "initramfs not found: $INITRAMFS_IMAGE_PATH"

case "$INITRAMFS_IMAGE_PATH" in
    *.cpio|*.cpio.gz) ;;
    *) die "unsupported initramfs format; use .cpio or .cpio.gz" ;;
esac

for tool in dtc fdtput make od qemu-system-riscv64 stat tr; do
    command -v "$tool" >/dev/null || die "required command not found: $tool"
done

RUN_LINUX_TMP=$(mktemp -d "${TMPDIR:-/tmp}/shadowfax-linux.XXXXXX")
trap 'rm -rf -- "$RUN_LINUX_TMP"' EXIT

LINUX_IMAGE_SIZE=$(stat -c %s "$LINUX_IMAGE_PATH")
INITRAMFS_IMAGE_SIZE=$(stat -c %s "$INITRAMFS_IMAGE_PATH")
LINUX_RUNTIME_SIZE=$(od -An -j 16 -N 8 -tu8 "$LINUX_IMAGE_PATH" | tr -d '[:space:]')
[[ "$LINUX_RUNTIME_SIZE" =~ ^[0-9]+$ ]] || die "cannot read the RISC-V Image runtime size"
(( LINUX_RUNTIME_SIZE >= LINUX_IMAGE_SIZE )) || LINUX_RUNTIME_SIZE=$LINUX_IMAGE_SIZE

LINUX_FILE_END_ADDR=$((LINUX_LOAD_ADDR + LINUX_IMAGE_SIZE))
LINUX_RUNTIME_END_ADDR=$((LINUX_LOAD_ADDR + LINUX_RUNTIME_SIZE))
INITRAMFS_LOAD_ADDR=$(((LINUX_RUNTIME_END_ADDR + INITRAMFS_ALIGNMENT - 1) & ~(INITRAMFS_ALIGNMENT - 1)))
INITRAMFS_END_ADDR=$((INITRAMFS_LOAD_ADDR + INITRAMFS_IMAGE_SIZE))

# Compile and patch a private DTB so its final size participates in validation.
PATCHED_DTB="$RUN_LINUX_TMP/device-tree.dtb"
dtc -I dts -O dtb \
    -o "$PATCHED_DTB" \
    "$PROJECT_ROOT/shadowfax/platform/$PLATFORM_NAME/device-tree.dts"

fdtput -t x "$PATCHED_DTB" /chosen linux,initrd-start \
    0 "0x$(printf '%x' "$INITRAMFS_LOAD_ADDR")"
fdtput -t x "$PATCHED_DTB" /chosen linux,initrd-end \
    0 "0x$(printf '%x' "$INITRAMFS_END_ADDR")"

FDT_IMAGE_SIZE=$(stat -c %s "$PATCHED_DTB")
FDT_END_ADDR=$((FDT_LOAD_ADDR + FDT_IMAGE_SIZE))

print_range "Supervisor RAM" \
    "$LINUX_LOAD_ADDR" "$SUPERVISOR_END_ADDR" "$SUPERVISOR_REGION_SIZE" \
    "Domain2: 32 MiB + 64 MiB regions"
print_range "Linux Image" \
    "$LINUX_LOAD_ADDR" "$LINUX_FILE_END_ADDR" "$LINUX_IMAGE_SIZE" "$LINUX_IMAGE_PATH"
print_range "Linux runtime" \
    "$LINUX_LOAD_ADDR" "$LINUX_RUNTIME_END_ADDR" "$LINUX_RUNTIME_SIZE" "Image header"
print_range "Initramfs" \
    "$INITRAMFS_LOAD_ADDR" "$INITRAMFS_END_ADDR" "$INITRAMFS_IMAGE_SIZE" "$INITRAMFS_IMAGE_PATH"
print_range "Device tree" \
    "$FDT_LOAD_ADDR" "$FDT_END_ADDR" "$FDT_IMAGE_SIZE" "$PATCHED_DTB"

check_no_overlap "Linux runtime" "$LINUX_LOAD_ADDR" "$LINUX_RUNTIME_END_ADDR" \
    "initramfs" "$INITRAMFS_LOAD_ADDR" "$INITRAMFS_END_ADDR"
check_no_overlap "Linux runtime" "$LINUX_LOAD_ADDR" "$LINUX_RUNTIME_END_ADDR" \
    "device tree" "$FDT_LOAD_ADDR" "$FDT_END_ADDR"
check_no_overlap "initramfs" "$INITRAMFS_LOAD_ADDR" "$INITRAMFS_END_ADDR" \
    "device tree" "$FDT_LOAD_ADDR" "$FDT_END_ADDR"

check_in_supervisor_region "Linux runtime" "$LINUX_LOAD_ADDR" "$LINUX_RUNTIME_END_ADDR"
check_in_supervisor_region "initramfs" "$INITRAMFS_LOAD_ADDR" "$INITRAMFS_END_ADDR"
check_in_supervisor_region "device tree" "$FDT_LOAD_ADDR" "$FDT_END_ADDR"

# Only build and start QEMU after the complete layout has passed validation.
make -B -C "$PROJECT_ROOT" PYTHON="$SHADOWFAX_PYTHON" PLATFORM="$PLATFORM_NAME" firmware

LINUX_QEMU_DEVICES="-device loader,file=$LINUX_IMAGE_PATH,addr=0x$(printf '%x' "$LINUX_LOAD_ADDR"),force-raw=on"
LINUX_QEMU_DEVICES+=" -device loader,file=$INITRAMFS_IMAGE_PATH,addr=0x$(printf '%x' "$INITRAMFS_LOAD_ADDR"),force-raw=on"
LINUX_QEMU_DEVICES+=" -netdev user,id=net0,ipv4=on,ipv6=off,hostfwd=tcp:127.0.0.1:$SSH_FORWARD_PORT-:22"
LINUX_QEMU_DEVICES+=" -device virtio-net-device,netdev=net0"

if [[ -n "${QEMU_DEVICES:-}" ]]; then
    LINUX_QEMU_DEVICES+=" $QEMU_DEVICES"
fi

printf 'Patched DTB : %s\n' "$PATCHED_DTB"
printf 'SSH forward : 127.0.0.1:%s -> guest:22\n' "$SSH_FORWARD_PORT"

make -C "$PROJECT_ROOT" \
    PYTHON="$SHADOWFAX_PYTHON" \
    PLATFORM="$PLATFORM_NAME" \
    FDT_IMAGE="$PATCHED_DTB" \
    QEMU_DEVICES="$LINUX_QEMU_DEVICES" \
    qemu-run
