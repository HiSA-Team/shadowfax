# zeroize-bench

Bare-metal RV64 program that measures how long it takes to zero 2 MiB to 4 GiB
with:

- `<[u8] as Zeroize>::zeroize`
- `core::ptr::write_bytes` + `zeroize::optimization_barrier`
- Zicboz `cbo.zero`

Each method runs with and without a Zicbom `cbo.clean` pass afterwards.

It is built the same way as Shadowfax (`riscv64imac-unknown-none-elf`,
`-C target-feature=+zicboz,+zicbom`) and runs in M-mode on QEMU `virt` with no
firmware:

    cargo run --release > results.log            # 5 runs, 2 MiB .. 4 GiB
    RUNS=3 MAX_MIB=256 cargo run --release       # smaller sweep

The output has one `method,clean,bytes,run,ticks` CSV row per run (ticks of the
10 MHz `mtime`), then a Markdown table of the medians in milliseconds.
`RUSTFLAGS` in the environment replaces the target features in
`.cargo/config.toml`, so unset it or pass the same features.

These are QEMU TCG timings, so they show relative cost, not hardware latency.
