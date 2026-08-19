# TVM launch latency

This standalone launcher creates and finalizes one 256 MiB TVM without running
it. It measures each COVH call and the sum of the startup COVH calls from the
untrusted host and emits CSV markers. The launcher copies a 32 MiB `0xa5`
payload using sixteen 2 MiB
measured-page calls, then maps the remaining 224 MiB as zero pages. The TVM is
intentionally left runnable because stopping it requires entering it; QEMU
termination reclaims the test state.

The runner writes `run,kind,operation,cycles,instructions,time_ticks` to
`results.csv`. `kind` is `covh` for an individual call or `tvm_startup` for the
sum of the measured startup calls. Progress messages are printed before each
call and are excluded from these measurements. Time ticks use the platform's
10 MHz timebase.
