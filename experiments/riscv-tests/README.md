# RISC-V tests benchmark experiments

`scripts/run-riscv-tests.sh` runs the five integer microbenchmarks used for the
IPC validation table:

- `median`;
- `memcpy`;
- `multiply`;
- `qsort`;
- `rsort`.

CoreMark is intentionally excluded. The runner evaluates two independent
comparison pairs:

- native M-mode versus bare-metal S-mode inside a TVM;
- native Linux versus Linux inside a TVM.

```sh
scripts/run-riscv-tests.sh 20
```

Each benchmark performs its existing preallocation pass before measuring the
real operation. The measured interval is read directly from the RISC-V `cycle`
and `instret` counters. Raw serial logs and one row per repetition are stored
in `results.csv` with the following columns:

```text
pair,mode,benchmark,run,cycles,instructions,ipc,status
```

Linux measurements use `perf_event_open` with the legacy RISC-V PMU configured
in `guests/linux/kernel.config`. This provides the same cycle and retired
instruction events without exposing the SBI PMU extension to the TVM.

The script does not calculate aggregates or standard deviations. Those should
be derived from the raw CSV by the experiment analysis code.
