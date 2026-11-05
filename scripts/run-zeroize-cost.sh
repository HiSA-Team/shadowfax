#!/usr/bin/env bash
# Measure what zeroing converted memory costs in COVH CONVERT_PAGES.
#
# For each zeroize variant the firmware is rebuilt with the matching RUSTFLAGS,
# then test/convert-zeroize-cost converts GUEST_RAM_SIZE bytes in one call for
# every size in SIZES. Each (variant, size) pair is booted RUNS times.
#
#   scripts/run-zeroize-cost.sh                 # 5 runs, both cbo.clean variants, 2M..4G
#   scripts/run-zeroize-cost.sh 10 out-dir      # 10 runs into out-dir
#   VARIANTS="zeroize zicboz" SIZES="2M 64M" scripts/run-zeroize-cost.sh 3
#
# Writes results.csv (one row per boot) and summary.md (median per variant and
# size, ready to paste in an issue) into the output directory.

set -euo pipefail

ROOT=$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
RUNS=${1:-5}
OUT=${2:-"$ROOT/experiments/convert-zeroize-cost/run-$(date -u +%Y%m%dT%H%M%SZ)"}
TARGET=riscv64imac-unknown-none-elf
PLATFORM_NAME=${PLATFORM:-generic}
# Release by default: a debug build zeroes byte by byte through an unoptimized
# loop, which says nothing about the cost of the algorithm itself.
export PROFILE=${PROFILE:-release}
PYTHON3=${PYTHON3:-python3}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-1800}
SIZES=${SIZES:-"2M 4M 8M 16M 32M 64M 128M 256M 512M 1024M 2048M 4096M"}
VARIANTS=${VARIANTS:-"zeroize+zicbom zicboz+zicbom"}

# The zeroize path in shadowfax/src/cove.rs is selected at compile time by
# these target features; everything else in the build is identical.
rustflags_for() {
    case $1 in
        zeroize)        echo "-C target-feature=+h" ;;
        zeroize+zicbom) echo "-C target-feature=+h,+zicbom" ;;
        zicboz)         echo "-C target-feature=+h,+zicboz" ;;
        zicboz+zicbom)  echo "-C target-feature=+h,+zicboz,+zicbom" ;;
        *) echo "unknown variant: $1 (zeroize, zeroize+zicbom, zicboz, zicboz+zicbom)" >&2; return 1 ;;
    esac
}

to_bytes() {
    local size=$1
    case $size in
        *G) echo $(( ${size%G} * 1024 * 1024 * 1024 )) ;;
        *M) echo $(( ${size%M} * 1024 * 1024 )) ;;
        *K) echo $(( ${size%K} * 1024 )) ;;
        *)  echo "$size" ;;
    esac
}

[[ $RUNS =~ ^[1-9][0-9]*$ ]] || {
    echo "usage: $0 [runs] [output-directory]" >&2
    echo "  SIZES=\"2M 4M ...\"        guest sizes to convert (default 2M..4096M)" >&2
    echo "  VARIANTS=\"zeroize ...\"   zeroize, zeroize+zicbom, zicboz, zicboz+zicbom" >&2
    echo "  PROFILE=debug|release    firmware build profile (default release)" >&2
    exit 2
}
[[ ! -e "$OUT" ]] || { echo "output already exists: $OUT" >&2; exit 1; }

for variant in $VARIANTS; do rustflags_for "$variant" >/dev/null; done
for tool in make qemu-system-riscv64 taskset timeout "$PYTHON3"; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
for file in "$ROOT/shadowfax/keys/privatekey.pem" \
            "$ROOT/shadowfax/keys/root_of_trust_priv.bin" \
            "$ROOT/shadowfax/keys/root_of_trust_pub.bin"; do
    [[ -f "$file" ]] || { echo "missing key: $file; run make generate-keys" >&2; exit 1; }
done

mkdir -p "$OUT/logs"

FW="$ROOT/target/$TARGET/$PROFILE/shadowfax"
DICE="$ROOT/target/artifacts/shadowfax.dice.bin"
TSM="$ROOT/target/artifacts/tsm.elf"
TSM_SIGNATURE="$ROOT/target/artifacts/tsm.bin.signature"
TEST_DIR="$ROOT/test/convert-zeroize-cost"
BUILD_DIR="$ROOT/target/convert-zeroize-cost"
LAUNCHER="$BUILD_DIR/convert-zeroize-cost.bin"
DTB="$BUILD_DIR/device-tree.dtb"

RESULTS="$OUT/results.csv"
echo 'variant,rustflags,profile,run,guest_ram_size,cycles,instructions,time_ticks' > "$RESULTS"

for variant in $VARIANTS; do
    rustflags=$(rustflags_for "$variant")
    echo "== $variant ($rustflags)"
    # The top-level Makefile stamps PROFILE and RUSTFLAGS, so changing them
    # rebuilds Shadowfax and the TSM; the TSM image size feeds the layout,
    # which the launcher rule picks up on its own.
    make -C "$ROOT" firmware PLATFORM="$PLATFORM_NAME" RUSTFLAGS="$rustflags" \
        >"$OUT/logs/build-$variant.log" 2>&1 || {
        echo "firmware build failed: $OUT/logs/build-$variant.log" >&2; exit 1;
    }

    for size in $SIZES; do
        bytes=$(to_bytes "$size")
        make -C "$TEST_DIR" GUEST_RAM_SIZE="$bytes" all \
            >"$OUT/logs/build-$variant-$size.log" 2>&1 || {
            echo "launcher build failed: $OUT/logs/build-$variant-$size.log" >&2; exit 1;
        }
        # shellcheck source=/dev/null
        source "$BUILD_DIR/layout.sh"

        for run in $(seq 1 "$RUNS"); do
            log="$OUT/logs/$variant-$size-run-$run.log"
            set +e
            # Detach stdin and drop the monitor: under `timeout` QEMU runs in a
            # background process group and would stop on SIGTTOU/SIGTTIN.
            timeout "$QEMU_TIMEOUT" taskset -c 0 qemu-system-riscv64 \
                -M virt -m "$SHADOWFAX_QEMU_MEM" -smp "$SHADOWFAX_SMP" -nographic \
                -monitor none -no-reboot \
                -bios "$FW" -dtb "$DTB" \
                -device loader,file="$DICE",addr="$SHADOWFAX_DICE_ADDR",force-raw=on \
                -device loader,file="$TSM",addr="$SHADOWFAX_TSM_IMAGE_ADDR",force-raw=on \
                -device loader,file="$TSM_SIGNATURE",addr="$SHADOWFAX_TSM_SIGNATURE_ADDR",force-raw=on \
                -device loader,file="$LAUNCHER",addr="$SHADOWFAX_DOMAIN_UNTRUSTED_DOMAIN_ENTRY",force-raw=on \
                </dev/null 2>&1 | tr -d '\r' >"$log"
            status=${PIPESTATUS[0]}
            set -e
            [[ $status -ne 124 ]] || echo "$variant $size run $run timed out after ${QEMU_TIMEOUT}s" >&2

            grep -q '^\[HOST\] PASS:' "$log" || {
                echo "$variant $size run $run did not complete: $log" >&2
                exit 1
            }
            row=$(sed -n 's/^ZEROIZE,//p' "$log")
            echo "$variant,\"$rustflags\",$PROFILE,$run,$row" >> "$RESULTS"
            printf '  %-14s %6s run %-3s %s\n' "$variant" "$size" "$run" "$row"
        done
    done
done

"$PYTHON3" - "$RESULTS" "$OUT/summary.md" <<'EOF'
import csv
import statistics
import sys

results, summary = sys.argv[1], sys.argv[2]
with open(results, newline="") as f:
    rows = list(csv.DictReader(f))

variants = list(dict.fromkeys(r["variant"] for r in rows))
sizes = sorted({int(r["guest_ram_size"]) for r in rows})
TIMEBASE_HZ = 10_000_000  # QEMU virt timebase-frequency


def median(variant, size, column):
    values = [int(r[column]) for r in rows
              if r["variant"] == variant and int(r["guest_ram_size"]) == size]
    return statistics.median(values) if values else None


def human(size):
    return f"{size >> 30} GiB" if size >= 1 << 30 else f"{size >> 20} MiB"


lines = [
    "Median `CONVERT_PAGES` latency in ms (wall time from `rdtime`), "
    f"{rows[0]['profile']} build, {max(int(r['run']) for r in rows)} runs each.",
    "",
    "| Size | " + " | ".join(variants) + " |",
    "|---:|" + "---:|" * len(variants),
]
for size in sizes:
    cells = []
    for v in variants:
        t = median(v, size, "time_ticks")
        cells.append("-" if t is None else f"{t * 1000 / TIMEBASE_HZ:.2f}")
    lines.append(f"| {human(size)} | " + " | ".join(cells) + " |")

lines += ["", "Median retired instructions:", "",
          "| Size | " + " | ".join(variants) + " |",
          "|---:|" + "---:|" * len(variants)]
for size in sizes:
    cells = []
    for v in variants:
        n = median(v, size, "instructions")
        cells.append("-" if n is None else f"{n:,.0f}")
    lines.append(f"| {human(size)} | " + " | ".join(cells) + " |")

with open(summary, "w") as f:
    f.write("\n".join(lines) + "\n")
print("\n".join(lines))
EOF

echo "Results: $RESULTS"
echo "Summary: $OUT/summary.md"
