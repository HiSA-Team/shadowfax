#!/usr/bin/env bash
set -e

PORT=$(cd "$(dirname "$0")" && pwd)
args=("$@")

for i in "${!args[@]}"; do
    if [[ ${args[$i]} == */src/cubic/libcubic.c ]]; then
        args[$i]="$PORT/cubic.c"
    fi
done

exec riscv64-unknown-elf-gcc "${args[@]}"
