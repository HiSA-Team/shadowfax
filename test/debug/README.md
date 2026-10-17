# GDB CoVE scenarios

These scripts turn GDB into a synthetic untrusted host. Instead of building a C
launcher, they write a short sequence of `ecall` instructions straight into the
boot domain's memory, break either side of each one, and assert on the registers
and memory the TSM leaves behind.

That makes them the right tool for questions a launcher answers badly: what
exactly does the TSM return for a malformed argument, what does a register hold
between two CoVE calls, where does a fault actually come from.

## Prerequisites

Put the RISC-V toolchain on `PATH` however you prefer, then:

```sh
uv venv && uv pip install -r requirements.txt && source .venv/bin/activate
make guests
make PLATFORM=generic firmware
```

You need a RISC-V GDB matching `RV_PREFIX`, plus `socat` for the `qemu-reset`
helper. `make build-info` prints the prefix in use.

## Two terminals

**Terminal 1 — start QEMU halted at reset.** `DEBUG=1` adds `-s -S` and a
monitor socket at `/tmp/shadowfax-qemu-monitor`:

```sh
make -C test/standalone-tvm-launcher run DEBUG=1
```

**Terminal 2 — attach.** The scripts need two addresses, and both are exported
by `scripts/layout.py` next to the device tree, so sourcing `layout.sh` is
enough (see [BUILD.md](../../BUILD.md)):

```sh
source target/standalone-tvm-launcher/layout.sh

BOOT_DOMAIN_ADDRESS=$SHADOWFAX_DOMAIN_RUNNER_ENTRY \
TSM_LOAD_ADDRESS=$SHADOWFAX_REGION_MONITOR_RAM_BASE \
  riscv64-unknown-elf-gdb -x test/debug/gdbinit \
    -x test/debug/gdb_covh_get_tsm_info.py \
    target/riscv64imac-unknown-none-elf/debug/shadowfax
```

- `BOOT_DOMAIN_ADDRESS` is where the scenario writes its instructions and data.
  It is the untrusted domain's entry, so the payload lands in memory that domain
  already owns.
- `TSM_LOAD_ADDRESS` is the trusted domain's base. The TSM is a
  position-independent ELF that Shadowfax relocates there, so **without this its
  symbols resolve at the wrong addresses** and every TSM breakpoint or backtrace
  is nonsense. `gdbinit` warns if it is unset.

The session stops at `tsm::main`. Type `continue` at each breakpoint the runner
installs:

```gdb
(gdb) continue
```

## Scenarios

| Script | What it drives |
| --- | --- |
| `gdb_covh_get_tsm_info.py` | `SUPD_GET_ACTIVE_DOMAINS`, then `COVH_GET_TSM_INFO`; asserts only the TSM domain is visible and the returned structure is 48 bytes |
| `gdb_covh_create_tvm.py` | Converts pages and creates a TVM from an inline payload |
| `gdb_covh_create_tvm_from_elf.py` | The same, loading the guest from an ELF |

## How a scenario is built

`test/riscv_tee.py` provides three pieces:

- **`Domain(name, instr_base, data_base)`** — one supervisor domain's scratch
  area. The scripts use `instr_base = BOOT_DOMAIN_ADDRESS` and
  `data_base = instr_base + 0x1000`.
- **`Step(name, regs, setup_mem_fn, assert_fn)`** — one `ecall`. `regs` is the
  register file to install before it (`a7` the extension, `a6` the function ID
  with the destination domain in bits 31:26), `setup_mem_fn` prepares memory
  first, and `assert_fn(prev, curr)` runs afterwards with snapshots from before
  and after the call.
- **`Runner`** — allocates an instruction slot per step, writes the `ecall` and
  a tail loop, and installs the paired breakpoints.

A minimal scenario is therefore:

```python
domain = Domain(name="untrusted", instr_base=addr, data_base=addr + 0x1000)
runner.add_step(
    Step(
        name="get_tsm_info",
        regs={"a0": domain.data_base, "a1": 48,
              "a6": (1 << 26) | COVH_GET_TSM_INFO, "a7": EID_COVH_ID},
        assert_fn=lambda prev, curr: check(curr["regs"]),
    ),
    domain,
)
runner.install_breakpoints()
```

An `assert_fn` that raises stops the inferior where it happened, so the failing
state is still on the machine when GDB drops back to the prompt.

## Useful bits

`qemu-reset`, defined in `gdbinit`, restarts the machine through the monitor
socket without leaving GDB:

```gdb
(gdb) qemu-reset
```

To inspect the platform's memory map from inside a session, the same exports are
available as JSON in `target/platform/generic/layout.json`.

## When something looks wrong

**TSM symbols point at nonsense.** `TSM_LOAD_ADDRESS` was unset or stale.
Re-source `layout.sh`; the trusted domain's base changes if you edit the tree.

**Execution runs away to address 0 and never returns.** The TSM sets `stvec` to
zero outside VM execution, so an unhandled S-mode fault becomes an endless
OpenSBI trap-redirect loop. Break on `sbi_trap_redirect` and read the trap
frame's `mepc` and `ra` to find the original fault:

```gdb
(gdb) break sbi_trap_redirect
(gdb) continue
(gdb) set $r = (struct sbi_trap_regs *) $a0
(gdb) printf "mepc=%#lx ra=%#lx\n", $r->mepc, $r->ra
(gdb) info symbol $r->ra
```

**`KeyError: 'BOOT_DOMAIN_ADDRESS'`.** The variable must be in the environment
of the `gdb` process, not set inside GDB.

**GDB cannot find the toolchain.** Put the RISC-V toolchain `bin/` on `PATH` first.
