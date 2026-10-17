#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
RUNS=${1:-3}
OUT=${2:-"$ROOT/experiments/tvm-launch-latency/run-$(date -u +%Y%m%dT%H%M%SZ)"}
TARGET=riscv64imac-unknown-none-elf
PLATFORM_NAME=${PLATFORM:-generic}
FW="$ROOT/target/$TARGET/debug/shadowfax"
DICE="$ROOT/target/artifacts/shadowfax.dice.bin"
TSM="$ROOT/target/artifacts/tsm.elf"
TSM_SIGNATURE="$ROOT/target/artifacts/tsm.bin.signature"
QEMU_TIMEOUT=${QEMU_TIMEOUT:-900}
LATENCY_DTB="$ROOT/target/tvm-launch-latency/device-tree.dtb"
# The experiment measures how long zeroing the guest region takes, so the guest
# size is the independent variable. It must still fit the guest region declared
# by test/tvm-launch-latency/device-tree.dts; ld fails the link if it does not.
GUEST_RAM_SIZE=${GUEST_RAM_SIZE:-33554432}

[[ $RUNS =~ ^[1-9][0-9]*$ ]] || {
    echo "usage: $0 [runs] [output-directory]" >&2
    echo "  GUEST_RAM_SIZE=<bytes>  guest RAM to create and zero (default 32 MiB)" >&2
    exit 2
}
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT" >&2; exit 1; }

for tool in make qemu-system-riscv64 taskset timeout; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
for file in "$ROOT/shadowfax/keys/privatekey.pem" \
            "$ROOT/shadowfax/keys/root_of_trust_priv.bin" \
            "$ROOT/shadowfax/keys/root_of_trust_pub.bin"; do
    [[ -f "$file" ]] || { echo "missing key: $file; run make generate-keys" >&2; exit 1; }
done

mkdir -p "$OUT/logs"

echo "Building firmware with the COVH TSM entry point"
make -C "$ROOT" firmware PLATFORM="$PLATFORM_NAME"

make -C "$ROOT/test/tvm-launch-latency" GUEST_RAM_SIZE="$GUEST_RAM_SIZE" all

# scripts/layout.py emits these next to the blob it produced.
# shellcheck source=/dev/null
source "$ROOT/target/tvm-launch-latency/layout.sh"
DICE_ADDR=$SHADOWFAX_DICE_ADDR
TSM_ADDR=$SHADOWFAX_TSM_IMAGE_ADDR
TSM_SIGNATURE_ADDR=$SHADOWFAX_TSM_SIGNATURE_ADDR
LOAD_ADDR=$SHADOWFAX_DOMAIN_UNTRUSTED_DOMAIN_ENTRY
LAUNCHER="$ROOT/target/tvm-launch-latency/tvm-launch-latency.bin"

RESULTS="$OUT/results.csv"
echo 'run,kind,operation,cycles,instructions,time_ticks' > "$RESULTS"

for run in $(seq 1 "$RUNS"); do
    log="$OUT/logs/run-$run.log"
    echo "[run $run/$RUNS]"
    set +e
    timeout "$QEMU_TIMEOUT" taskset -c 0 qemu-system-riscv64 \
        -M virt -m "$SHADOWFAX_QEMU_MEM" -smp "$SHADOWFAX_SMP" -nographic \
        -bios "$FW" -dtb "$LATENCY_DTB" \
        -device loader,file="$DICE",addr="$DICE_ADDR",force-raw=on \
        -device loader,file="$TSM",addr="$TSM_ADDR",force-raw=on \
        -device loader,file="$TSM_SIGNATURE",addr="$TSM_SIGNATURE_ADDR",force-raw=on \
        -device loader,file="$LAUNCHER",addr="$LOAD_ADDR",force-raw=on \
        2>&1 | tee "$log" | sed -u -n \
        's/\r$//; /^LATENCY,/p; /^\[HOST\]/p'

    # The OpenSBI console emits \r\n; normalize before parsing numeric columns.
    tr -d '\r' <"$log" >"$log.tmp" && mv "$log.tmp" "$log"

    grep -q '^\[HOST\] PASS:' "$log" || {
        echo "run $run did not complete: $log" >&2
        exit 1
    }
    sed -n 's/^LATENCY,//p' "$log" | sed "s/^/$run,/" >> "$RESULTS"
done

echo "Results: $RESULTS"
