#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
RUNS=${1:-3}
OUT=${2:-"$ROOT/experiments/riscv-tests/run-$(date -u +%Y%m%dT%H%M%SZ)"}
TARGET=riscv64imac-unknown-none-elf
BENCHMARKS=(median memcpy multiply qsort rsort)
SUITE="$ROOT/guests/bare-metal/riscv-tests/benchmarks"
BASE_INITRD="$ROOT/bin/linux-tvm-initramfs.cpio.gz"
KERNEL_IMAGE="$ROOT/linux/guest/arch/riscv/boot/Image"
KERNEL_ELF="$ROOT/linux/guest/vmlinux"
WORK=$(mktemp -d /tmp/shadowfax-riscv-tests.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

source "$ROOT/config.rc"

[[ $RUNS =~ ^[1-9][0-9]*$ ]] || { echo "usage: $0 [runs] [output-directory]"; exit 2; }
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT"; exit 1; }
mkdir -p "$OUT/logs" "$WORK/m-mode" "$WORK/s-mode" "$WORK/linux" "$WORK/rootfs"

COMMON_FLAGS=(
    -I"$SUITE/../env" -I"$SUITE/common"
    -U_FORTIFY_SOURCE -DPREALLOCATE=1 -mcmodel=medany -static -std=gnu99
    -O2 -ffast-math -fno-common -fno-builtin-printf
    -fno-tree-loop-distribute-patterns -Wno-implicit-int
    -Wno-implicit-function-declaration -mabi=lp64 -march=rv64imac -g
)

echo "Building RISC-V test benchmarks"
for benchmark in "${BENCHMARKS[@]}"; do
    sources=("$SUITE/$benchmark/"*.c)

    riscv64-unknown-elf-gcc "${COMMON_FLAGS[@]}" -I"$SUITE/$benchmark" \
        -o "$WORK/m-mode/$benchmark" "${sources[@]}" \
        "$SUITE/common/syscalls.c" "$SUITE/common/crt.S" \
        -static -nostdlib -nostartfiles -lm -lgcc -T "$SUITE/common/test.ld" \
        >"$OUT/logs/$benchmark-m-mode-build.log" 2>&1

    riscv64-unknown-elf-gcc "${COMMON_FLAGS[@]}" -I"$SUITE/$benchmark" -DSMODE \
        -o "$WORK/s-mode/$benchmark" "${sources[@]}" \
        "$SUITE/common/syscalls.c" "$SUITE/common/crt.S" \
        -static -nostdlib -nostartfiles -lm -lgcc -T "$SUITE/common/cove.ld" \
        >"$OUT/logs/$benchmark-s-mode-build.log" 2>&1

    riscv64-unknown-linux-gnu-gcc "${COMMON_FLAGS[@]}" -I"$SUITE/$benchmark" \
        -Dmemcpy=riscv_test_memcpy -o "$WORK/linux/$benchmark" \
        "${sources[@]}" "$SUITE/common/linux.c" \
        >"$OUT/logs/$benchmark-linux-build.log" 2>&1
done

echo 'pair,mode,benchmark,run,cycles,instructions,ipc,status' >"$OUT/results.csv"
touch "$WORK/empty"

for benchmark in "${BENCHMARKS[@]}"; do
    for run in $(seq 1 "$RUNS"); do
        echo "[m-mode] $benchmark run $run/$RUNS"
        log="$OUT/logs/m-mode-$benchmark-$run.log"
        taskset -c 0 qemu-system-riscv64 -M virt -m 1G -smp 1 -nographic \
            -monitor none -no-reboot -bios "$WORK/m-mode/$benchmark" >"$log" 2>&1

        cycles=$(sed -n -E 's/^m?cycle = ([0-9]+).*/\1/p' "$log" | tail -1)
        instructions=$(sed -n -E 's/^m?instret = ([0-9]+).*/\1/p' "$log" | tail -1)
        status=$(sed -n -E 's/^status = ([0-9]+).*/\1/p' "$log" | tail -1)
        [[ -n $cycles && -n $instructions && $status == 0 ]] || { echo "Benchmark failed: $log"; exit 1; }
        ipc=$(awk -v i="$instructions" -v c="$cycles" 'BEGIN { printf "%.9f", i / c }')
        echo "bare-metal,m-mode,$benchmark,$run,$cycles,$instructions,$ipc,$status" | tee -a "$OUT/results.csv"
    done

    echo "Embedding S-mode $benchmark TVM"
    TSM_GUEST_ELF="$WORK/s-mode/$benchmark" \
        TSM_GUEST_DTB="$WORK/empty" TSM_GUEST_INITRD="$WORK/empty" \
        RUSTFLAGS='-C target-feature=+h' \
        cargo build --quiet --manifest-path "$ROOT/Cargo.toml" --target "$TARGET" \
        -p tsm --features standalone >"$OUT/logs/$benchmark-tsm-build.log" 2>&1

    for run in $(seq 1 "$RUNS"); do
        echo "[s-mode-tvm] $benchmark run $run/$RUNS"
        log="$OUT/logs/s-mode-tvm-$benchmark-$run.log"
        taskset -c 0 qemu-system-riscv64 -M virt -m 1G -smp 1 -nographic \
            -monitor none -no-reboot -kernel "$ROOT/target/$TARGET/debug/tsm" >"$log" 2>&1

        cycles=$(sed -n -E 's/^m?cycle = ([0-9]+).*/\1/p' "$log" | tail -1)
        instructions=$(sed -n -E 's/^m?instret = ([0-9]+).*/\1/p' "$log" | tail -1)
        status=$(sed -n -E 's/^status = ([0-9]+).*/\1/p' "$log" | tail -1)
        [[ -n $cycles && -n $instructions && $status == 0 ]] || { echo "Benchmark failed: $log"; exit 1; }
        ipc=$(awk -v i="$instructions" -v c="$cycles" 'BEGIN { printf "%.9f", i / c }')
        echo "bare-metal,s-mode-tvm,$benchmark,$run,$cycles,$instructions,$ipc,$status" | tee -a "$OUT/results.csv"
    done
done

(
    cd "$WORK/rootfs"
    gzip -dc "$BASE_INITRD" | cpio -idmu --quiet
)
mkdir -p "$WORK/rootfs/opt/riscv-tests"
cp "$WORK/linux/"* "$WORK/rootfs/opt/riscv-tests/"

cat >"$WORK/rootfs/etc/init.d/riscv-tests" <<EOF
#!/bin/sh
mode=unknown
for arg in \$(cat /proc/cmdline); do
    case \$arg in riscv-tests.mode=*) mode=\${arg#riscv-tests.mode=};; esac
done
for benchmark in ${BENCHMARKS[*]}; do
    run=1
    while [ \$run -le $RUNS ]; do
        echo "RISCV_TEST_RUN,\$mode,\$benchmark,\$run,$RUNS"
        "/opt/riscv-tests/\$benchmark" >/tmp/riscv-test.out 2>&1
        status=\$?
        cat /tmp/riscv-test.out
        cycles=\$(sed -n -E 's/^cycle = ([0-9]+).*/\1/p' /tmp/riscv-test.out)
        instructions=\$(sed -n -E 's/^instret = ([0-9]+).*/\1/p' /tmp/riscv-test.out)
        echo "RISCV_TEST_RESULT,linux,\$mode,\$benchmark,\$run,\$cycles,\$instructions,\$status"
        run=\$((run + 1))
    done
done
sync
sleep 1
poweroff -f
EOF
chmod +x "$WORK/rootfs/etc/init.d/riscv-tests"
cat >"$WORK/rootfs/etc/inittab" <<'EOF'
::sysinit:/etc/init.d/rcS
::once:/etc/init.d/riscv-tests
EOF

INITRD="$WORK/riscv-tests-initramfs.cpio.gz"
(
    cd "$WORK/rootfs"
    find . -print0 | cpio --null -o --format=newc --quiet | gzip -9 >"$INITRD"
)

NATIVE_LOG="$OUT/logs/linux-native.log"
echo "[linux-native] running all benchmarks"
taskset -c 0 qemu-system-riscv64 -M virt -m 256M -smp 1 -nographic \
    -monitor none -no-reboot -kernel "$KERNEL_IMAGE" -initrd "$INITRD" \
    -append 'console=ttyS0,115200 riscv-tests.mode=linux-native' \
    2>&1 | tee "$NATIVE_LOG" | sed -u -n 's/\r$//; /^RISCV_TEST_RUN,/p; /^RISCV_TEST_RESULT,/p'

TVM_DTB="$WORK/linux-tvm.dtb"
dtc -q -I dts -O dtb -o "$TVM_DTB" "$ROOT/guests/linux/linux-tvm.dts"
INITRD_START=$((0x01000000))
INITRD_END=$((INITRD_START + $(stat -c %s "$INITRD")))
fdtput -t x "$TVM_DTB" /chosen linux,initrd-start 0 "0x$(printf '%x' "$INITRD_START")"
fdtput -t x "$TVM_DTB" /chosen linux,initrd-end 0 "0x$(printf '%x' "$INITRD_END")"
fdtput -t s "$TVM_DTB" /chosen bootargs 'earlycon=uart8250,mmio,0x18000000 console=ttyS0,115200 riscv-tests.mode=linux-tvm'

echo "Embedding Linux RISC-V tests TVM"
TSM_GUEST_ELF="$KERNEL_ELF" TSM_GUEST_DTB="$TVM_DTB" \
    TSM_GUEST_INITRD="$INITRD" RUSTFLAGS='-C target-feature=+h' \
    cargo build --quiet --manifest-path "$ROOT/Cargo.toml" --target "$TARGET" \
    -p tsm --features standalone >"$OUT/logs/linux-tsm-build.log" 2>&1

TVM_LOG="$OUT/logs/linux-tvm.log"
echo "[linux-tvm] running all benchmarks"
taskset -c 0 qemu-system-riscv64 -M virt -m 1G -smp 1 -nographic \
    -monitor none -no-reboot -kernel "$ROOT/target/$TARGET/debug/tsm" \
    2>&1 | tee "$TVM_LOG" | sed -u -n 's/\r$//; /^RISCV_TEST_RUN,/p; /^RISCV_TEST_RESULT,/p'

for log in "$NATIVE_LOG" "$TVM_LOG"; do
    count=$(tr -d '\r' <"$log" | grep -c '^RISCV_TEST_RESULT,' || true)
    expected=$((RUNS * ${#BENCHMARKS[@]}))
    [[ $count -eq $expected ]] || { echo "$log produced $count/$expected results"; exit 1; }

    while IFS=, read -r pair mode benchmark run cycles instructions status; do
        [[ -n $cycles && -n $instructions && $status == 0 ]] || { echo "Benchmark failed: $log"; exit 1; }
        ipc=$(awk -v i="$instructions" -v c="$cycles" 'BEGIN { printf "%.9f", i / c }')
        echo "$pair,$mode,$benchmark,$run,$cycles,$instructions,$ipc,$status" >>"$OUT/results.csv"
    done < <(tr -d '\r' <"$log" | sed -n 's/^RISCV_TEST_RESULT,//p')
done

echo "Results: $OUT/results.csv"
