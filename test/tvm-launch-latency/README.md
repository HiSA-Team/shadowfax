# TVM launch latency

This standalone launcher creates and finalizes one TVM without running it. It
measures each COVH call and the sum of the startup COVH calls from the
untrusted host and emits CSV markers. The launcher copies a 16 MiB `0xa5`
payload using eight 2 MiB measured-page calls, then maps whatever remains of
the guest region as zero pages. The TVM is intentionally left runnable because
stopping it requires entering it; QEMU termination reclaims the test state.

`GUEST_RAM_SIZE` defaults to 32 MiB, the smallest useful size: `PAYLOAD_SIZE`
in `main.c` is a fixed 16 MiB, so a smaller TVM would ask for zero zero-pages
and the TSM rejects that. The guest region lives in this test's own
`device-tree.dts`, which overrides `umem-high` and `/memory`; raise both it and
`GUEST_RAM_SIZE` together to measure a larger TVM, keeping the region aligned
to its own size.

`scripts/run-tvm-creation-latency.sh` drives the experiment and takes
`GUEST_RAM_SIZE` from the environment, so the size can be swept without editing
anything: `GUEST_RAM_SIZE=$((16*1024*1024)) scripts/run-tvm-creation-latency.sh 5`.
It takes QEMU's `-m` and `-smp` from this test's own `layout.sh`, so widening
`/memory` in the tree is enough -- there is no memory size in the script. A
guest larger than the region fails at link time with `region 'GUEST' overflowed`.

The runner writes `run,kind,operation,cycles,instructions,time_ticks` to
`results.csv`. `kind` is `covh` for an individual call or `tvm_startup` for the
sum of the measured startup calls. Progress messages are printed before each
call and are excluded from these measurements. Time ticks use the platform's
10 MHz timebase.
