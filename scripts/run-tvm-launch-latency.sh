#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
RUNS=${1:-3}
OUT=${2:-"$ROOT/experiments/tvm-launch-latency/run-$(date -u +%Y%m%dT%H%M%SZ)"}
TARGET=riscv64imac-unknown-none-elf
PLATFORM_NAME=${PLATFORM:-generic}
FW="$ROOT/target/$TARGET/debug/shadowfax"
DICE="$ROOT/bin/shadowfax.dice.bin"
BASE_DTB="$ROOT/bin/$PLATFORM_NAME/device-tree.dtb"
QEMU_TIMEOUT=${QEMU_TIMEOUT:-900}
WORK=$(mktemp -d /tmp/shadowfax-tvm-launch-latency.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

[[ $RUNS =~ ^[1-9][0-9]*$ ]] || {
    echo "usage: $0 [runs] [output-directory]" >&2
    exit 2
}
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT" >&2; exit 1; }

for tool in fdtget fdtput make qemu-system-riscv64 taskset timeout; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
for file in "$ROOT/shadowfax/keys/privatekey.pem" \
            "$ROOT/shadowfax/keys/root_of_trust_priv.bin" \
            "$ROOT/shadowfax/keys/root_of_trust_pub.bin"; do
    [[ -f "$file" ]] || { echo "missing key: $file; run make generate-keys" >&2; exit 1; }
done

mkdir -p "$OUT/logs"

echo "Building firmware with the COVH TSM entry point"
make -B -C "$ROOT" firmware PLATFORM="$PLATFORM_NAME" PYTHON="uv run --with cbor2"

cp "$BASE_DTB" "$WORK/platform.dtb"
fdtput -t x "$WORK/platform.dtb" /chosen/opensbi-domains/umem-high base 0 0xa0000000
fdtput -t x "$WORK/platform.dtb" /chosen/opensbi-domains/umem-high order 0x1c

make -C "$ROOT/test/tvm-launch-latency" DTB="$WORK/platform.dtb" GUEST_RAM_SIZE=33554432 all

read -r dice_hi dice_lo < <(fdtget -t x "$WORK/platform.dtb" /chosen/shadowfax dice-input)
DICE_ADDR=$((16#$dice_hi << 32 | 16#$dice_lo))
read -r load_hi load_lo < <(fdtget -t x "$WORK/platform.dtb" /chosen/opensbi-domains/untrusted-domain next-addr)
LOAD_ADDR=$((16#$load_hi << 32 | 16#$load_lo))
LAUNCHER="$ROOT/target/tvm-launch-latency/tvm-launch-latency.bin"

RESULTS="$OUT/results.csv"
echo 'run,kind,operation,cycles,instructions,time_ticks' > "$RESULTS"

for run in $(seq 1 "$RUNS"); do
    log="$OUT/logs/run-$run.log"
    echo "[run $run/$RUNS]"
    set +e
    taskset -c 0 qemu-system-riscv64 \
        -M virt -m 1G -smp 1 -nographic \
        -bios "$FW" -dtb "$WORK/platform.dtb" \
        -device loader,file="$DICE",addr="$DICE_ADDR",force-raw=on \
        -device loader,file="$LAUNCHER",addr="$LOAD_ADDR",force-raw=on \
        2>&1 | tee "$log" | sed -u -n \
        's/\r$//; /^LATENCY,/p; /^\[HOST\]/p'
    set -e

    # The OpenSBI console emits \r\n; normalize before parsing numeric columns.
    tr -d '\r' <"$log" >"$log.tmp" && mv "$log.tmp" "$log"

    covh_metrics=$(grep -c '^LATENCY,covh,' "$log" || true)
    startup_metrics=$(grep -c '^LATENCY,tvm_startup,' "$log" || true)
    [[ $covh_metrics -eq 23 && $startup_metrics -eq 1 ]] || {
        echo "run $run produced $covh_metrics/23 COVH and $startup_metrics/1 startup metrics: $log" >&2
        exit 1
    }
    grep -q '^\[HOST\] PASS:' "$log" || {
        echo "run $run did not complete: $log" >&2
        exit 1
    }
    sed -n 's/^LATENCY,//p' "$log" | sed "s/^/$run,/" >> "$RESULTS"
done

echo "Results: $RESULTS"
