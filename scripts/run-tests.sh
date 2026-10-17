#!/usr/bin/env bash
# Run every test that boots QEMU and shuts it down on its own.
#
# Discovers launchers by looking for a Makefile under test/, so a new scenario
# is picked up without editing this script. Each one must print a
# "[HOST] PASS:" or "[HOST] Program completed" line; anything else, a panic, a
# non-zero exit or a timeout counts as a failure.
#
#   scripts/run-tests.sh              # everything
#   scripts/run-tests.sh trust two    # only tests whose path matches a pattern
#   TIMEOUT=600 scripts/run-tests.sh  # raise the per-test limit
#
# Run `make firmware` first; this script builds launchers but not the platform.

set -uo pipefail

ROOT=$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
TIMEOUT=${TIMEOUT:-600}
LOG_DIR=${LOG_DIR:-$ROOT/target/test-logs}
HOST_TRIPLET=$(rustc -vV | awk '/^host:/ { print $2 }')

mkdir -p "$LOG_DIR"

matches() {
    [[ $# -eq 1 ]] && return 0
    local candidate=$1; shift
    local pattern
    for pattern in "$@"; do
        [[ $candidate == *"$pattern"* ]] && return 0
    done
    return 1
}

names=()
outcomes=()
seconds=()
failed=0

record() {
    names+=("$1")
    outcomes+=("$2")
    seconds+=("$3")
    [[ $2 == pass ]] || failed=1
    printf '%-34s %-8s %4ss\n' "$1" "$2" "$3"
}

printf '%-34s %-8s %5s\n' "TEST" "RESULT" "TIME"

# The functional test boots the staged firmware and checks it reaches S-mode.
if matches "functional" "$@"; then
    start=$SECONDS
    log=$LOG_DIR/functional.log
    if cargo test --manifest-path "$ROOT/test/functional/Cargo.toml" \
            --target "$HOST_TRIPLET" >"$log" 2>&1; then
        record functional pass $((SECONDS - start))
    else
        record functional FAIL $((SECONDS - start))
    fi
fi

for makefile in "$ROOT"/test/*/Makefile "$ROOT"/test/security/*/Makefile; do
    [[ -f $makefile ]] || continue
    directory=$(dirname "$makefile")
    name=${directory#"$ROOT"/test/}
    matches "$name" "$@" || continue

    log=$LOG_DIR/${name//\//-}.log
    start=$SECONDS
    timeout "$TIMEOUT" make -C "$directory" run </dev/null >"$log" 2>&1
    status=$?
    elapsed=$((SECONDS - start))

    if [[ $status -eq 124 ]]; then
        record "$name" TIMEOUT "$elapsed"
    elif [[ $status -ne 0 ]]; then
        record "$name" FAIL "$elapsed"
    elif grep -q 'panicked at' "$log"; then
        record "$name" PANIC "$elapsed"
    elif tr -d '\r' <"$log" | grep -qE '^\[HOST\] (PASS|Program completed)'; then
        record "$name" pass "$elapsed"
    else
        record "$name" NOPASS "$elapsed"
    fi
done

echo
if [[ $failed -ne 0 ]]; then
    echo "Failures:"
    for index in "${!names[@]}"; do
        [[ ${outcomes[$index]} == pass ]] && continue
        echo "  ${names[$index]}: ${outcomes[$index]} -- $LOG_DIR/${names[$index]//\//-}.log"
    done
    exit 1
fi

echo "All ${#names[@]} tests passed. Logs in $LOG_DIR"
