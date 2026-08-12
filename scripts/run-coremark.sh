#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
RUNS=${1:-3}
OUT=${2:-"$ROOT/experiments/coremark/run-$(date -u +%Y%m%dT%H%M%SZ)"}
TARGET=riscv64imac-unknown-none-elf
ITERATIONS=30000
COREMARK="$ROOT/guests/bare-metal/coremark"
BASE_INITRD="$ROOT/bin/linux-tvm-initramfs.cpio.gz"
KERNEL_IMAGE="$ROOT/linux/guest/arch/riscv/boot/Image"
KERNEL_ELF="$ROOT/linux/guest/vmlinux"
WORK=$(mktemp -d /tmp/shadowfax-coremark.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

source "$ROOT/config.rc"

[[ $RUNS =~ ^[1-9][0-9]*$ ]] || { echo "usage: $0 [runs] [output-directory]"; exit 2; }
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT"; exit 1; }
mkdir -p "$OUT/logs" "$WORK/m-mode/riscv" "$WORK/s-mode/riscv" "$WORK/linux" "$WORK/rootfs"

echo "Building M-mode CoreMark"
make -s -C "$COREMARK" PORT_DIR=riscv RV_PREFIX=riscv64-unknown-elf- \
    OPATH="$WORK/m-mode/" SMODE=0 ITERATIONS=$ITERATIONS link \
    >"$OUT/logs/m-mode-build.log" 2>&1

echo "Building S-mode CoreMark"
make -s -C "$COREMARK" PORT_DIR=riscv RV_PREFIX=riscv64-unknown-elf- \
    OPATH="$WORK/s-mode/" SMODE=1 ITERATIONS=$ITERATIONS link \
    >"$OUT/logs/s-mode-build.log" 2>&1

echo "Building Linux CoreMark"
make -s -C "$COREMARK" PORT_DIR=linux CC=riscv64-unknown-linux-gnu-gcc \
    OPATH="$WORK/linux/" ITERATIONS=$ITERATIONS \
    XCFLAGS='-static -march=rv64imafdc -mabi=lp64 -DSEED_METHOD=SEED_VOLATILE' link \
    >"$OUT/logs/linux-build.log" 2>&1

touch "$WORK/empty"
TSM_GUEST_ELF="$WORK/s-mode/coremark.bin" \
    TSM_GUEST_DTB="$WORK/empty" TSM_GUEST_INITRD="$WORK/empty" \
    RUSTFLAGS='-C target-feature=+h' \
    cargo build --quiet --manifest-path "$ROOT/Cargo.toml" --target "$TARGET" \
    -p tsm --features standalone >"$OUT/logs/tsm-build.log" 2>&1

echo 'pair,mode,run,total_ticks,tick_hz,benchmark_seconds,iterations,iterations_per_second,verification_status' >"$OUT/results.csv"

for mode in m-mode s-mode-tvm; do
    for run in $(seq 1 "$RUNS"); do
        echo "[$mode] run $run/$RUNS"
        log="$OUT/logs/$mode-$run.log"

        if [[ $mode == m-mode ]]; then
            taskset -c 0 qemu-system-riscv64 -M virt -m 1G -smp 1 -nographic \
                -monitor none -no-reboot -bios "$WORK/m-mode/coremark.bin" >"$log" 2>&1
        else
            taskset -c 0 qemu-system-riscv64 -M virt -m 1G -smp 1 -nographic \
                -monitor none -no-reboot -kernel "$ROOT/target/$TARGET/debug/tsm" >"$log" 2>&1
        fi

        tr -d '\r' <"$log" >"$WORK/result.log"
        grep -q 'Correct operation validated' "$WORK/result.log" || { echo "CoreMark failed: $log"; exit 1; }
        ticks=$(sed -n 's/^Total ticks *: *//p' "$WORK/result.log" | tail -1)
        seconds=$(sed -n 's/^Total time (secs): *//p' "$WORK/result.log" | tail -1)
        iterations=$(sed -n 's/^Iterations *: *//p' "$WORK/result.log" | tail -1)
        rate=$(sed -n 's/^Iterations\/Sec *: *//p' "$WORK/result.log" | tail -1)
        [[ $iterations -eq $ITERATIONS ]] || { echo "Unexpected iteration count: $log"; exit 1; }
        echo "bare-metal,$mode,$run,$ticks,10000000,$seconds,$iterations,$rate,0" | tee -a "$OUT/results.csv"
    done
done

(
    cd "$WORK/rootfs"
    gzip -dc "$BASE_INITRD" | cpio -idmu --quiet
)
mkdir -p "$WORK/rootfs/opt"
cp "$WORK/linux/coremark.exe" "$WORK/rootfs/opt/coremark"
cat >"$WORK/rootfs/etc/init.d/coremark" <<EOF
#!/bin/sh
mode=unknown
for arg in \$(cat /proc/cmdline); do
    case \$arg in coremark.mode=*) mode=\${arg#coremark.mode=};; esac
done
run=1
while [ \$run -le $RUNS ]; do
    echo "COREMARK_RUN,\$mode,\$run,$RUNS"
    /opt/coremark >/tmp/coremark.out 2>&1
    status=\$?
    cat /tmp/coremark.out
    ticks=\$(sed -n 's/^Total ticks *: *//p' /tmp/coremark.out)
    seconds=\$(sed -n 's/^Total time (secs): *//p' /tmp/coremark.out)
    iterations=\$(sed -n 's/^Iterations *: *//p' /tmp/coremark.out)
    rate=\$(sed -n 's/^Iterations\/Sec *: *//p' /tmp/coremark.out)
    grep -q 'Correct operation validated' /tmp/coremark.out || status=1
    [ "\$iterations" -eq $ITERATIONS ] || status=1
    echo
    echo "COREMARK_RESULT,linux,\$mode,\$run,\$ticks,1000,\$seconds,\$iterations,\$rate,\$status"
    run=\$((run + 1))
done
sync
sleep 1
poweroff -f
EOF
chmod +x "$WORK/rootfs/etc/init.d/coremark"
cat >"$WORK/rootfs/etc/inittab" <<'EOF'
::sysinit:/etc/init.d/rcS
::once:/etc/init.d/coremark
EOF

INITRD="$WORK/coremark-initramfs.cpio.gz"
(
    cd "$WORK/rootfs"
    find . -print0 | cpio --null -o --format=newc --quiet | gzip -9 >"$INITRD"
)

NATIVE_LOG="$OUT/logs/linux-native.log"
echo "[linux-native] $RUNS run(s)"
taskset -c 0 qemu-system-riscv64 -M virt -m 256M -smp 1 -display none \
    -monitor none -nographic -no-reboot -kernel "$KERNEL_IMAGE" -initrd "$INITRD" \
    -append 'console=ttyS0,115200 coremark.mode=linux-native' \
    2>&1 | tee "$NATIVE_LOG" | sed -u -n 's/\r$//; /^COREMARK_RUN,/p; /^COREMARK_RESULT,/p'

NATIVE_RESULTS=$(tr -d '\r' <"$NATIVE_LOG" | grep -c '^COREMARK_RESULT,' || true)
[[ $NATIVE_RESULTS -eq $RUNS ]] || { echo "Native Linux produced $NATIVE_RESULTS/$RUNS results"; exit 1; }
NATIVE_VALID=$(tr -d '\r' <"$NATIVE_LOG" | grep -c '^COREMARK_RESULT,.*0$' || true)
[[ $NATIVE_VALID -eq $RUNS ]] || { echo "Native Linux validation failed"; exit 1; }
tr -d '\r' <"$NATIVE_LOG" | sed -n 's/^COREMARK_RESULT,//p' >>"$OUT/results.csv"

TVM_DTB="$WORK/linux-tvm.dtb"
dtc -q -I dts -O dtb -o "$TVM_DTB" "$ROOT/guests/linux/linux-tvm.dts"
INITRD_START=$((0x01000000))
INITRD_END=$((INITRD_START + $(stat -c %s "$INITRD")))
fdtput -t x "$TVM_DTB" /chosen linux,initrd-start 0 "0x$(printf '%x' "$INITRD_START")"
fdtput -t x "$TVM_DTB" /chosen linux,initrd-end 0 "0x$(printf '%x' "$INITRD_END")"
fdtput -t s "$TVM_DTB" /chosen bootargs 'earlycon=uart8250,mmio,0x18000000 console=ttyS0,115200 coremark.mode=linux-tvm'

echo "Embedding Linux CoreMark TVM"
TSM_GUEST_ELF="$KERNEL_ELF" TSM_GUEST_DTB="$TVM_DTB" \
    TSM_GUEST_INITRD="$INITRD" RUSTFLAGS='-C target-feature=+h' \
    cargo build --quiet --manifest-path "$ROOT/Cargo.toml" --target "$TARGET" \
    -p tsm --features standalone >"$OUT/logs/linux-tsm-build.log" 2>&1

TVM_LOG="$OUT/logs/linux-tvm.log"
echo "[linux-tvm] $RUNS run(s)"
taskset -c 0 qemu-system-riscv64 -M virt -m 1G -smp 1 -nographic \
    -monitor none -no-reboot -kernel "$ROOT/target/$TARGET/debug/tsm" \
    2>&1 | tee "$TVM_LOG" | sed -u -n 's/\r$//; /^COREMARK_RUN,/p; /^COREMARK_RESULT,/p'

TVM_RESULTS=$(tr -d '\r' <"$TVM_LOG" | grep -c '^COREMARK_RESULT,' || true)
[[ $TVM_RESULTS -eq $RUNS ]] || { echo "Linux TVM produced $TVM_RESULTS/$RUNS results"; exit 1; }
TVM_VALID=$(tr -d '\r' <"$TVM_LOG" | grep -c '^COREMARK_RESULT,.*0$' || true)
[[ $TVM_VALID -eq $RUNS ]] || { echo "Linux TVM validation failed"; exit 1; }
tr -d '\r' <"$TVM_LOG" | sed -n 's/^COREMARK_RESULT,//p' >>"$OUT/results.csv"

echo "Results: $OUT/results.csv"
