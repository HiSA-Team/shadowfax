# Guest workloads

Shadowfax supports two different classes of TVM workload:

- `bare-metal/` contains freestanding test and attestation workloads.
- `linux/` contains the source configuration for the Linux TVM guest.

Build the bare-metal workloads with:

```sh
make -C guests bare-metal
```

The Linux TVM guest is not built by the default `guests` target because it requires external Linux
and BusyBox source trees. See [linux/README.md](linux/README.md) for its build and boot workflow.

CoreMark and RISC-V tests are vendored workloads. Keep changes to those directories narrowly scoped
to the Shadowfax port.
