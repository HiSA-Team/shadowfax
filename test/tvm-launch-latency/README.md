# TVM launch latency

This standalone launcher creates and finalizes one TVM without running it. It
measures each COVH call and the sum of the startup COVH calls from the
untrusted host and emits CSV markers. A measured `0xa5` payload stands in for a kernel
and initramfs: half of the guest, at most 16 MiB, about the size of a microVM
kernel and initrd. It is copied with one
measured-page call per 2 MiB chunk (a single smaller call for a 2 MiB guest);
one zero-page call maps the rest of the guest. `results.csv` has one
`ADD_MEASURED_PAGES` row per chunk. The TVM is
intentionally left runnable because stopping it requires entering it; QEMU
termination reclaims the test state.

The page directory is never smaller than the TSM's 16 KiB minimum. The guest
region is 1 GiB, which is also the most the TSM's Sv39x4 tables
(`tsm/src/gpt.rs`) can map.

`scripts/run-tvm-creation-latency.sh [runs-per-size] [output-directory]` sweeps
the guest size from 2 MiB to 1 GiB on the release firmware, running each size `runs-per-size` times (default 10). For
each size it writes `dts/<size>MiB.dts`, which includes this tree and
shrinks `umem-high` to exactly the guest: a guest converted out of a larger
region leaves the host one NAPOT region per power of two of remainder, and at
small sizes the host's and the TSM's regions together overflow
`MAX_MEMORY_REGIONS` while `CREATE_TVM` holds the bootstrap grant.

All runs go to one `results.csv` with columns
`seq,run,guest_ram_size,kind,operation,cycles,instructions,time_ticks`. `seq`
numbers every QEMU run across the sweep, `run` restarts at 1 for each size, and
logs are `logs/<seq>-<size>MiB-run-<run>.log`. `kind` is `covh` for an
individual call or `tvm_startup` for
the sum of the measured startup calls. Progress messages are printed before
each call and are excluded from these measurements. Time ticks use the
platform's 10 MHz timebase. `experiments/analyze.py` plots the latest sweep.
