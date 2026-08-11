# RV8 benchmark

This experiment runs the eight upstream RV8 programs under two matched Linux environments:

- `native`: the guest Linux `Image` runs directly on QEMU `virt` with OpenSBI;
- `tvm`: the same kernel and initramfs run as a standalone confidential TVM.

Both modes use one CPU, 256 MiB of Linux-visible RAM, the same statically linked `-O2`
`rv64imafdc/lp64` binaries, and the same initramfs. The guest BusyBox `time` applet records process
wall, user, and system time. These are QEMU measurements, not hardware cycle measurements.
QEMU is pinned to host logical CPU 0.
The guest kernel configuration enables futexes because the statically linked C++ `bigint` program
uses them through libstdc++.

Initialize submodules and build the guest Linux artifacts described in `guests/linux/README.md`, then
run three repetitions with:

```sh
scripts/run-rv8.sh 3
```

Each run creates one directory here containing `results.csv` and the two serial logs. Commit a run
directory unchanged when retaining measurements. The console prints one `RV8_RESULT` line for every
completed iteration; `Ctrl-C` stops QEMU and leaves the full partial serial log in that directory.
