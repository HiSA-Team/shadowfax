# CoreMark experiments

`scripts/run-coremark.sh` runs two separate CoreMark comparisons:

- M-mode bare metal versus S-mode bare metal inside a TVM;
- native Linux versus Linux inside a TVM.

```sh
scripts/run-coremark.sh 3
```

Raw serial logs and `results.csv` are written under a timestamped directory in
`experiments/coremark/`. Every environment executes 30,000 iterations.

The same Linux executable, kernel, and generated initramfs are used for native
Linux and TVM Linux. The executable is installed as `/opt/coremark`, and its
init script runs it repeatedly before powering off. Bare-metal ticks use the
10 MHz RISC-V time counter; Linux ticks use the POSIX port's 1 kHz counter.

CoreMark floating-point reporting is enabled for the bare-metal port, so both
elapsed seconds and iterations per second retain fractional precision. Compare
only the two modes belonging to the same `pair` in `results.csv`.
