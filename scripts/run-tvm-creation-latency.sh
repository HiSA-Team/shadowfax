#!/usr/bin/env bash
# Measure TVM-creation latency for guests from 2 MiB to 1 GiB on the release
# firmware. Every run of every size lands in one results.csv.
set -euo pipefail

ROOT=$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
RUNS=${1:-10}
OUT=${2:-"$ROOT/experiments/tvm-launch-latency/sweep-$(date -u +%Y%m%dT%H%M%SZ)"}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-3600}
export PROFILE=release

# Powers of two: each one is the order of the guest's umem-high region.
# 1 GiB is the most the TSM's Sv39x4 tables (tsm/src/gpt.rs) can map.
SIZES_MIB=(2 4 8 16 32 64 128 256 512 1024)

FW="$ROOT/target/riscv64imac-unknown-none-elf/$PROFILE/shadowfax"
ARTIFACTS="$ROOT/target/artifacts"
BUILD="$ROOT/target/tvm-launch-latency"

[[ $RUNS =~ ^[1-9][0-9]*$ ]] || { echo "usage: $0 [runs-per-size] [output-directory]" >&2; exit 2; }
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT" >&2; exit 1; }

# Shrink umem-high to exactly the guest. A guest converted out of a larger
# region leaves the host one NAPOT piece per power of two of remainder; for
# small guests those pieces overflow the PMP map during CREATE_TVM.
write_dts() {
    local size_mib=$1 order=20
    while (( (1 << (order - 20)) < size_mib )); do order=$((order + 1)); done
    cat <<EOF
/include/ "test/tvm-launch-latency/device-tree.dts"

&umem_high {
	order = <$order>;
};
EOF
}

run_qemu() {
    # Detach stdin and drop the monitor: under `timeout` QEMU runs in a
    # background process group and would stop on SIGTTOU touching the terminal.
    timeout "$QEMU_TIMEOUT" taskset -c 0 qemu-system-riscv64 \
        -M virt -m "$SHADOWFAX_QEMU_MEM" -smp "$SHADOWFAX_SMP" -nographic \
        -monitor none -no-reboot \
        -bios "$FW" -dtb "$BUILD/device-tree.dtb" \
        -device loader,file="$ARTIFACTS/shadowfax.dice.bin",addr="$SHADOWFAX_DICE_ADDR",force-raw=on \
        -device loader,file="$ARTIFACTS/tsm.elf",addr="$SHADOWFAX_TSM_IMAGE_ADDR",force-raw=on \
        -device loader,file="$ARTIFACTS/tsm.bin.signature",addr="$SHADOWFAX_TSM_SIGNATURE_ADDR",force-raw=on \
        -device loader,file="$BUILD/tvm-launch-latency.bin",addr="$SHADOWFAX_DOMAIN_UNTRUSTED_DOMAIN_ENTRY",force-raw=on \
        </dev/null
}

mkdir -p "$OUT/logs" "$OUT/dts"
make -C "$ROOT" firmware PLATFORM="${PLATFORM:-generic}"

RESULTS="$OUT/results.csv"
echo 'seq,run,guest_ram_size,kind,operation,cycles,instructions,time_ticks' > "$RESULTS"

seq_no=0
total=$(( ${#SIZES_MIB[@]} * RUNS ))
for size_mib in "${SIZES_MIB[@]}"; do
    guest_ram_size=$(( size_mib << 20 ))
    # Built from a fixed path: the layout's dependency file records it, so a
    # path inside $OUT would break the next sweep once $OUT is removed.
    mkdir -p "$BUILD"
    write_dts "$size_mib" > "$BUILD/sweep.dts"
    cp "$BUILD/sweep.dts" "$OUT/dts/${size_mib}MiB.dts"
    make -C "$ROOT/test/tvm-launch-latency" GUEST_RAM_SIZE="$guest_ram_size" \
        LAUNCHER_DTS="$BUILD/sweep.dts" all
    # shellcheck source=/dev/null
    source "$BUILD/layout.sh"

    for run in $(seq 1 "$RUNS"); do
        seq_no=$((seq_no + 1))
        log="$OUT/logs/$(printf '%03d' "$seq_no")-${size_mib}MiB-run-$run.log"
        echo "[$seq_no/$total] ${size_mib} MiB, run $run/$RUNS"

        # OpenSBI prints \r\n; strip it before the log is parsed.
        run_qemu 2>&1 | tr -d '\r' > "$log" || true
        grep -q '^\[HOST\] PASS:' "$log" || { echo "run failed: $log" >&2; exit 1; }

        sed -n "s/^LATENCY,/$seq_no,$run,$guest_ram_size,/p" "$log" >> "$RESULTS"
    done
done

echo "Results: $RESULTS"
