# Debugging Shadowfax

Shadowfax provides GDB Python scenarios that act as a synthetic untrusted host. They issue CoVE SBI
calls step by step, inspect registers and memory, and assert the TSM's responses.

## Prerequisites

Build the firmware, TSM symbols, guest workloads, signatures, and DICE input before starting GDB:

```sh
make -B PYTHON='uv run --with cbor2'
```

Use a RISC-V GDB matching `RV_PREFIX`. The default GDB initialization loads symbols from:

```text
target/riscv64imac-unknown-none-elf/debug/shadowfax
target/riscv64imac-unknown-none-elf/debug/tsm
```

## Two-terminal workflow

Start QEMU halted at reset in the first terminal:

```sh
make qemu-run DEBUG=1 PYTHON='uv run --with cbor2'
```

`DEBUG=1` enables the GDB server on TCP port 1234, stops the CPUs before execution, and exposes a
QEMU monitor socket at `/tmp/shadowfax-qemu-monitor`.

In a second terminal, attach GDB and select a scenario:

```sh
make debug GDB_COVE_SCRIPT=test/debug/gdb_covh_get_tsm_info.py
```

The session initially stops at `tsm::main`. Type `continue` each time the Python runner reaches the
next programmed breakpoint:

```gdb
(gdb) continue
```

Keep the QEMU terminal visible because firmware and guest console output appears there.

## Available scenarios

### Query TSM information

```sh
make debug GDB_COVE_SCRIPT=test/debug/gdb_covh_get_tsm_info.py
```

This enumerates active supervisor domains, requests the TSM information structure, reads it from
untrusted memory, and validates the reported state and TVM capabilities.

### Create a synthetic TVM

```sh
make debug GDB_COVE_SCRIPT=test/debug/gdb_covh_create_tvm.py
```

This scenario donates pages, creates a TVM and memory region, installs a minimal loop as measured
guest code, creates a vCPU, finalizes the TVM, and enters it.

### Create a TVM from an ELF

```sh
make debug GDB_COVE_SCRIPT=test/debug/gdb_covh_create_tvm_from_elf.py
```

This is the debugger-driven counterpart of the standalone launcher. It reads
`guests/bare-metal/attestation.out`, allocates confidential memory, maps each loadable ELF segment
into guest physical memory, creates the vCPU, and runs the attestation guest. The script requires
GDB's Python environment to provide `pyelftools`.

For a complete demonstration that does not require GDB, prefer:

```sh
make -C test/standalone-tvm-launcher/ run
```

### Boot the Linux TVM guest directly

This is different from debugging the Linux untrusted host with `scripts/run-linux.sh`. The
standalone TSM embeds `linux/guest/vmlinux`, `bin/linux-tvm.dtb`, and
`bin/linux-tvm-initramfs.cpio.gz`, then enters Linux as a confidential VS-mode guest:

```sh
cargo build --target riscv64imac-unknown-none-elf -p tsm
qemu-system-riscv64 -M virt -nographic -smp 1 -m 512M \
    -kernel target/riscv64imac-unknown-none-elf/debug/tsm
```

Early Linux output uses the TVM UART described at guest GPA `0x05000000`. Page-loading diagnostics
come from the TSM before the Linux console takes over. Use the configuration and artifact-generation
instructions in [`guests/linux/README.md`](guests/linux/README.md).

## Useful GDB commands

The supplied `test/debug/gdbinit` defines `qemu-reset`, which resets QEMU through its monitor socket
when `socat` is installed:

```gdb
(gdb) qemu-reset
```

Standard commands remain useful around SBI and domain transitions:

```gdb
(gdb) info registers
(gdb) x/16gx $sp
(gdb) bt
(gdb) continue
```

Restart both QEMU and GDB when changing firmware addresses or rebuilding debug symbols.

## Functional boot test

The non-interactive integration test builds the firmware and verifies its QEMU boot output:

```sh
make test PYTHON='uv run --with cbor2'
```

Use this as a regression check after debugging a firmware or domain-layout change.
