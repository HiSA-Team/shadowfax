# Two TVMs test

This standalone test embeds one guest ELF and creates two TVMs from it. Each
TVM receives a different `entry_arg` through `FINALIZE_TVM`; the guest reads
that value from `a1`, prints it, and shuts down.

The launcher runs the TVMs sequentially:

```text
[TVM] id 0x1
[TVM] id 0x2
```

Build and run it after building the firmware inputs:

```sh
make -C test/two-tvms all
make -C test/two-tvms run
```

Each TVM has separate metadata and 16 MiB guest RAM. The launcher destroys
both TVMs and reclaims all four converted memory ranges before printing the
pass message.
