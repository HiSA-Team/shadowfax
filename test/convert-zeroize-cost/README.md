# CONVERT_PAGES zeroize cost

This launcher converts `GUEST_RAM_SIZE` bytes of host memory to confidential
memory with a single COVH `CONVERT_PAGES` call, prints one
`ZEROIZE,<bytes>,<cycles>,<instructions>,<time_ticks>` line and shuts QEMU
down. Shadowfax zeroes the converted range inside that call, so the latency
tracks the zeroize implementation the firmware was built with
(`zeroize()` in `shadowfax/src/cove.rs`):

| Variant         | `RUSTFLAGS`                            | Zeroing                         |
|-----------------|----------------------------------------|---------------------------------|
| `zeroize`       | `-C target-feature=+h`                 | `zeroize` crate, byte stores    |
| `zeroize+zicbom` | `-C target-feature=+h,+zicbom`        | `zeroize` + `cbo.clean` per block |
| `zicboz`        | `-C target-feature=+h,+zicboz`         | `cbo.zero` per block            |
| `zicboz+zicbom` | `-C target-feature=+h,+zicboz,+zicbom` | `cbo.zero` + `cbo.clean`        |

The guest region in `device-tree.dts` is fixed at 4 GiB (based at 4 GiB), so
any `GUEST_RAM_SIZE` from 2 MiB to 4 GiB fits the same tree. Converting the
whole sweep touches up to 4 GiB of host memory in QEMU.

`scripts/run-zeroize-cost.sh` rebuilds the firmware for each variant and sweeps
2 MiB to 4 GiB. By default it runs only the two variants that write the zeros
back with `cbo.clean` (`zeroize+zicbom`, `zicboz+zicbom`): without it a
non-coherent DMA master can still read the previous owner's data from DRAM.

    scripts/run-zeroize-cost.sh 5          # 5 boots per variant and size
    VARIANTS="zeroize zicboz" SIZES="2M 64M" scripts/run-zeroize-cost.sh 3

It writes `results.csv` (`variant,rustflags,profile,run,guest_ram_size,cycles,
instructions,time_ticks`, one row per boot) and `summary.md` (medians as
Markdown tables) under `experiments/convert-zeroize-cost/`. Time ticks use the
platform's 10 MHz timebase. The firmware is built in release by default;
set `PROFILE=debug` to override.
