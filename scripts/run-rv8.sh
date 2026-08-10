#!/usr/bin/env bash
set -euo pipefail

# global variables
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
RUNS=${1:-3}
OUT=${2:-"$ROOT/experiments/rv8/run-$(date -u +%Y%m%dT%H%M%SZ)"}
RV_PREFIX=${RV_PREFIX:-riscv64-unknown-linux-gnu-}
RUSTFLAGS=${RUSTFLAGS:--C target-feature=+h}
TARGET=riscv64imac-unknown-none-elf
BENCHMARKS=(aes bigint dhrystone miniz norx primes qsort sha512)

[[ $RUNS =~ ^[1-9][0-9]*$ ]] || { echo "usage: $0 [runs] [output-directory]" >&2; exit 2; }

for tool in "${RV_PREFIX}gcc" "${RV_PREFIX}g++" cargo cpio dtc fdtput gzip qemu-system-riscv64 taskset; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done

# benchmark artifacts: rv8 suite, initramfs, kernel image, kernel elf
RV8="$ROOT/guests/linux/rv8-bench"
BASE_INITRD="$ROOT/bin/linux-tvm-initramfs.cpio.gz"
KERNEL_IMAGE="$ROOT/linux/guest/arch/riscv/boot/Image"
KERNEL_ELF="$ROOT/linux/guest/vmlinux"
[[ -f "$RV8/src/aes.c" ]] || { echo "initialize submodules first" >&2; exit 1; }
for file in "$BASE_INITRD" "$KERNEL_IMAGE" "$KERNEL_ELF"; do
    [[ -f "$file" ]] || { echo "missing artifact: $file" >&2; exit 1; }
done

# setup temporary working directory
WORK=$(mktemp -d /tmp/shadowfax-rv8.XXXXXX)
trap 'rm -rf "$WORK"' EXIT
trap 'printf "\nInterrupted. Partial logs are in %s\n" "$OUT" >&2; exit 130' INT
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT" >&2; exit 1; }
mkdir -p "$WORK/bin" "$WORK/rootfs" "$OUT"

# Build the Linux applications once. Both environments use these exact files.
for benchmark in aes dhrystone miniz norx primes qsort sha512; do
    echo CC $benchmark
    "${RV_PREFIX}gcc" -static -O2 -g -fPIE -std=gnu17 -march=rv64imafdc -mabi=lp64 \
        "$RV8/src/$benchmark.c" -o "$WORK/bin/$benchmark"
done

echo CXX $benchmark
"${RV_PREFIX}g++" -static -O2 -g -fPIE -march=rv64imafdc -mabi=lp64 \
    "$RV8/src/bigint.cc" -o "$WORK/bin/bigint"

# prepare benchmark executor and put it in rootfs
(
    cd "$WORK/rootfs"
    gzip -dc "$BASE_INITRD" | cpio -idmu --quiet
)
mkdir -p "$WORK/rootfs/opt/rv8"
cp "$WORK/bin/"* "$WORK/rootfs/opt/rv8/"

# Make the shared initramfs run every benchmark and print machine-readable markers.
cat >"$WORK/rootfs/etc/init.d/rv8" <<EOF
#!/bin/sh
mode=unknown
for arg in \$(cat /proc/cmdline); do
    case \$arg in rv8.mode=*) mode=\${arg#rv8.mode=};; esac
done
for benchmark in ${BENCHMARKS[*]}; do
    run=1
    while [ \$run -le $RUNS ]; do
        /bin/busybox time -o /tmp/rv8.time -f "%e,%U,%S" "/opt/rv8/\$benchmark"
        status=\$?
        echo "RV8_RESULT,\$mode,\$benchmark,\$run,\$(cat /tmp/rv8.time),\$status"
        run=\$((run + 1))
    done
done
sync
sleep 1
poweroff -f
EOF
chmod +x "$WORK/rootfs/etc/init.d/rv8"
cat >"$WORK/rootfs/etc/inittab" <<'EOF'
::sysinit:/etc/init.d/rcS
::once:/etc/init.d/rv8
EOF

INITRD="$WORK/rv8-initramfs.cpio.gz"
(
    cd "$WORK/rootfs"
    find . -print0 | cpio --null -o --format=newc --quiet | gzip -9 >"$INITRD"
)

EXPECTED_RESULTS=$((RUNS * ${#BENCHMARKS[@]}))
NATIVE_LOG="$OUT/native.log"

# Baseline: OpenSBI starts Linux directly. There is no TSM in this command.
echo "===== NATIVE LINUX: direct QEMU, host CPU 0 =====" | tee "$NATIVE_LOG"
taskset -c 0 qemu-system-riscv64 \
    -M virt -m 256M -smp 1 -display none -monitor none \
    -chardev stdio,id=console,signal=on -serial chardev:console -no-reboot \
    -kernel "$KERNEL_IMAGE" -initrd "$INITRD" \
    -append "console=ttyS0,115200 rv8.mode=native" \
    2>&1 | tee -a "$NATIVE_LOG" | sed -u -n 's/\r$//; /^RV8_RESULT,/p'

NATIVE_RESULTS=$(tr -d '\r' <"$NATIVE_LOG" | grep -c '^RV8_RESULT,' || true)
[[ $NATIVE_RESULTS -eq $EXPECTED_RESULTS ]] || {
    echo "Native Linux produced $NATIVE_RESULTS/$EXPECTED_RESULTS results" >&2
    exit 1
}
echo "===== NATIVE LINUX COMPLETE: $NATIVE_RESULTS results =====" | tee -a "$NATIVE_LOG"

TVM_DTB="$WORK/linux-tvm.dtb"
dtc -I dts -O dtb -o "$TVM_DTB" "$ROOT/guests/linux/linux-tvm.dts"
INITRD_START=$((0x01000000))
INITRD_END=$((INITRD_START + $(stat -c %s "$INITRD")))
fdtput -t x "$TVM_DTB" /chosen linux,initrd-start 0 "0x$(printf '%x' "$INITRD_START")"
fdtput -t x "$TVM_DTB" /chosen linux,initrd-end 0 "0x$(printf '%x' "$INITRD_END")"

if ! TSM_GUEST_ELF="$KERNEL_ELF" \
    TSM_GUEST_DTB="$TVM_DTB" \
    TSM_GUEST_INITRD="$INITRD" \
    RUSTFLAGS="$RUSTFLAGS" \
    cargo build --quiet --manifest-path "$ROOT/Cargo.toml" --target "$TARGET" \
        -p tsm --features standalone >"$WORK/tsm-build.log" 2>&1; then
    cat "$WORK/tsm-build.log" >&2
    exit 1
fi

TVM_LOG="$OUT/tvm.log"
# Comparison: QEMU starts the TSM, which then starts the same Linux userspace.
echo "===== TVM LINUX: QEMU -> TSM -> guest Linux, host CPU 0 =====" | tee "$TVM_LOG"
taskset -c 0 qemu-system-riscv64 \
    -M virt -m 1G -smp 1 -display none -monitor none \
    -chardev stdio,id=console,signal=on -serial chardev:console -no-reboot \
    -kernel "$ROOT/target/$TARGET/debug/tsm" \
    2>&1 | tee -a "$TVM_LOG" | sed -u -n 's/\r$//; /^RV8_RESULT,/p'

TVM_RESULTS=$(tr -d '\r' <"$TVM_LOG" | grep -c '^RV8_RESULT,' || true)
[[ $TVM_RESULTS -eq $EXPECTED_RESULTS ]] || {
    echo "TVM Linux produced $TVM_RESULTS/$EXPECTED_RESULTS results" >&2
    exit 1
}
echo "===== TVM LINUX COMPLETE: $TVM_RESULTS results =====" | tee -a "$TVM_LOG"

RESULTS="$OUT/results.csv"
# Serial logs are the raw data. Copy only result markers into the CSV.
echo 'mode,benchmark,run,real_seconds,user_seconds,sys_seconds,exit_code' >"$RESULTS"
tr -d '\r' <"$NATIVE_LOG" | sed -n 's/^RV8_RESULT,//p' >>"$RESULTS"
tr -d '\r' <"$TVM_LOG" | sed -n 's/^RV8_RESULT,//p' >>"$RESULTS"
echo "Results: $RESULTS"
