# Embench-IoT experiments

`scripts/run-embench.sh` compares the Embench-IoT 1.0 speed benchmarks in two
bare-metal environments:

- `m-mode`: the benchmark is the QEMU `virt` machine's native firmware;
- `s-mode`: the same source and compiler flags run as a bare-metal TVM through the
  standalone TSM, without Linux in either environment.

The M-mode linker and MMIO addresses come from the Shadowfax platform device
tree. The bare-metal TVM intentionally has no device tree: its ELF is linked at
the standalone TSM's existing `0x200000` guest-RAM base. The runner uses
upstream Embench's `build_all.py`, pins QEMU to host
CPU 0, prints progress before every run, and writes raw serial logs plus a CSV
file under a timestamped directory.

Initialize submodules and run:

```sh
git submodule update --init guests/bare-metal/embench-iot
scripts/run-embench.sh 3
```

Pass a second argument to choose an output directory suitable for committing:

```sh
scripts/run-embench.sh 10 experiments/embench/qemu-10-runs
```

Lower cycle counts are better. A zero `verification_status` means the Embench
workload produced its expected result. QEMU TCG cycle counters are useful for
controlled comparisons of these two configurations, but they are not physical
hardware measurements.

Both modes link the same small support library from
`guests/bare-metal/embench/`. This avoids using the toolchain's medlow C library
inside the M-mode image at `0x80000000`. The port also builds Embench 1.0's
`cubic` solver with `double` instead of `long double`, avoiding medlow quad-
precision helpers from libgcc. These choices are identical in both modes, but
the resulting cycle counts should not be compared to an official Embench board
baseline.
