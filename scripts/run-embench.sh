#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
RUNS=${1:-3}
OUT=${2:-"$ROOT/experiments/embench/run-$(date -u +%Y%m%dT%H%M%SZ)"}
TARGET=riscv64imac-unknown-none-elf
CPU=0

EMBENCH="$ROOT/guests/bare-metal/embench-iot"
PORT="$ROOT/guests/bare-metal/embench"
CC="$PORT/cc.sh"
WORK=$(mktemp -d /tmp/shadowfax-embench.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

[[ $RUNS =~ ^[1-9][0-9]*$ ]] || { echo "usage: $0 [runs] [output-directory]"; exit 2; }
[[ -f "$EMBENCH/build_all.py" ]] || { echo "initialize the Embench submodule"; exit 1; }
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT"; exit 1; }
mkdir -p "$OUT/logs"

dtc -q -I dts -O dtb "$ROOT/platform/generic/device-tree.dts" -o "$WORK/platform.dtb"
read -r _ M_RAM _ M_SIZE < <(fdtget -t x "$WORK/platform.dtb" /memory@80000000 reg)
read -r _ M_UART _ _ < <(fdtget -t x "$WORK/platform.dtb" /soc/serial@10000000 reg)
read -r _ M_TEST _ _ < <(fdtget -t x "$WORK/platform.dtb" /soc/test@100000 reg)

CFLAGS=(-O2 -g -std=gnu17 -ffunction-sections -fdata-sections -fno-common
    -march=rv64imafdc -mabi=lp64 -mcmodel=medany -msmall-data-limit=0 -I"$PORT")

mkdir -p "$WORK/m-mode" "$WORK/s-mode"
$CC -c "${CFLAGS[@]}" -fno-builtin "$PORT/lib.c" -o "$WORK/lib.o"
$CC -c "${CFLAGS[@]}" -DEMBENCH_UART_ADDR=0x$M_UART -DEMBENCH_TEST_ADDR=0x$M_TEST \
    "$PORT/start.S" -o "$WORK/m-mode/start.o"
$CC -c "${CFLAGS[@]}" -DEMBENCH_UART_ADDR=0x$M_UART -DEMBENCH_TEST_ADDR=0x$M_TEST \
    "$PORT/runtime.c" -o "$WORK/m-mode/runtime.o"
$CC -c "${CFLAGS[@]}" -DEMBENCH_SMODE -DEMBENCH_UART_ADDR=0x18000000 \
    "$PORT/start.S" -o "$WORK/s-mode/start.o"
$CC -c "${CFLAGS[@]}" -DEMBENCH_SMODE -DEMBENCH_UART_ADDR=0x18000000 \
    "$PORT/runtime.c" -o "$WORK/s-mode/runtime.o"

echo "Building M-mode firmware images"
python3 "$EMBENCH/build_all.py" --arch riscv32 --chip generic --board ri5cyverilator \
    --builddir "$WORK/m-mode/build" --logdir "$OUT/logs/m-mode-build" --clean --verbose \
    --cc "$CC" --ld "$CC" --cpu-mhz 10 --warmup-heat 1 --timeout 60 \
    --cflags="-c ${CFLAGS[*]}" \
    --ldflags="-nostdlib -static -Wl,--gc-sections -Wl,--wrap=start_trigger -Wl,--wrap=stop_trigger -Wl,--defsym=EMBENCH_RAM_BASE=0x$M_RAM -Wl,--defsym=EMBENCH_RAM_SIZE=0x$M_SIZE -T$PORT/linker.ld" \
    --user-libs="$WORK/m-mode/start.o $WORK/m-mode/runtime.o $WORK/lib.o -lgcc" \
    >"$OUT/logs/m-mode-build.log" 2>&1

echo "Building S-mode guest images"
python3 "$EMBENCH/build_all.py" --arch riscv32 --chip generic --board ri5cyverilator \
    --builddir "$WORK/s-mode/build" --logdir "$OUT/logs/s-mode-build" --clean --verbose \
    --cc "$CC" --ld "$CC" --cpu-mhz 10 --warmup-heat 1 --timeout 60 \
    --cflags="-c ${CFLAGS[*]}" \
    --ldflags="-nostdlib -static -Wl,--gc-sections -Wl,--wrap=start_trigger -Wl,--wrap=stop_trigger -Wl,--defsym=EMBENCH_RAM_BASE=0x200000 -Wl,--defsym=EMBENCH_RAM_SIZE=0x1000000 -T$PORT/linker.ld" \
    --user-libs="$WORK/s-mode/start.o $WORK/s-mode/runtime.o $WORK/lib.o -lgcc" \
    >"$OUT/logs/s-mode-build.log" 2>&1

mapfile -t BENCHMARKS < <(find "$EMBENCH/src" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort)
for benchmark in "${BENCHMARKS[@]}"; do
    [[ -x "$WORK/m-mode/build/src/$benchmark/$benchmark" ]] || { echo "M-mode build failed: $benchmark"; exit 1; }
    [[ -x "$WORK/s-mode/build/src/$benchmark/$benchmark" ]] || { echo "S-mode build failed: $benchmark"; exit 1; }
done

echo 'mode,benchmark,run,cycles,verification_status' >"$OUT/results.csv"
touch "$WORK/empty"

for benchmark in "${BENCHMARKS[@]}"; do
    for run in $(seq 1 "$RUNS"); do
        echo "[M-mode] $benchmark $run/$RUNS"
        log="$OUT/logs/m-mode-$benchmark-$run.log"
        taskset -c "$CPU" qemu-system-riscv64 -M virt -m 512M -smp 1 -nographic \
            -monitor none -no-reboot -bios "$WORK/m-mode/build/src/$benchmark/$benchmark" \
            >"$log" 2>&1
        result=$(tr -d '\r' <"$log" | sed -n 's/^EMBENCH_RESULT,//p' | tail -1)
        [[ -n $result ]] || { echo "no result: $log"; exit 1; }
        echo "m-mode,$benchmark,$run,$result" | tee -a "$OUT/results.csv"
        [[ ${result##*,} == 0 ]] || exit 1
    done

    echo "[S-mode] embedding $benchmark"
    TSM_GUEST_ELF="$WORK/s-mode/build/src/$benchmark/$benchmark" \
        TSM_GUEST_DTB="$WORK/empty" TSM_GUEST_INITRD="$WORK/empty" RUSTFLAGS='-C target-feature=+h' \
        cargo build --quiet --manifest-path "$ROOT/Cargo.toml" --target "$TARGET" \
        -p tsm --features standalone >"$OUT/logs/tsm-build-$benchmark.log" 2>&1

    for run in $(seq 1 "$RUNS"); do
        echo "[S-mode] $benchmark $run/$RUNS"
        log="$OUT/logs/s-mode-$benchmark-$run.log"
        taskset -c "$CPU" qemu-system-riscv64 -M virt -m 1G -smp 1 -nographic \
            -monitor none -no-reboot -kernel "$ROOT/target/$TARGET/debug/tsm" \
            >"$log" 2>&1
        result=$(tr -d '\r' <"$log" | sed -n 's/^EMBENCH_RESULT,//p' | tail -1)
        [[ -n $result ]] || { echo "no result: $log"; exit 1; }
        echo "s-mode,$benchmark,$run,$result" | tee -a "$OUT/results.csv"
        [[ ${result##*,} == 0 ]] || exit 1
    done
done

echo "Results: $OUT/results.csv"
