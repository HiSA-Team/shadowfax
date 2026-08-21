# Shadowfax

> [!WARNING]
> Shadowfax is an early-stage research project. Interfaces and memory layouts may change.

Shadowfax is an open-source RISC-V confidential-computing firmware stack based on the
Application-TEE (CoVE) specification. It combines an M-mode TSM driver, OpenSBI, and a trusted
security monitor (TSM) capable of creating and running trusted virtual machines (TVMs).

The firmware and TSM are tested with the `riscv64imac` target, RISC-V Privileged ISA v1.12,
OpenSBI v1.7, and QEMU's `virt` machine. The experimental Linux TVM guest additionally exposes
the F and D extensions to its userspace.

## Documentation index

- [Run the attestation example](#run-the-attestation-example): go from a clone to a bare-metal
  host and TVM demonstration.
- [Setup](SETUP.md): detailed host dependencies, toolchains, keys, Linux, initramfs, Docker, and
  musl instructions.
- [Linux TVM guest](#boot-linux-as-a-tvm-guest): boot Linux inside a confidential VM.
- [Linux host](#boot-linux-as-the-untrusted-host): boot Linux in the untrusted supervisor domain.
- [Multiple TSMs](MULTIPLE_TSMS.md): configure built-in and externally staged relocatable TSMs
  in separate trusted supervisor domains.
- [DEBUG.md](DEBUG.md): QEMU/GDB startup, synthetic CoVE-H scenarios, and debugger commands.

## Run the attestation example

The shortest working path launches a bare-metal CoVE host, creates a bare-metal TVM, and prints
the layered attestation evidence. Clone with submodules and install the dependencies in
[SETUP.md](SETUP.md):

```sh
git clone --recurse-submodules https://github.com/HiSA-Team/shadowfax
cd shadowfax
```

If this is a linked Git worktree, initialize its submodules before building:

```sh
git submodule update --init --recursive
```

Generate development keys once, build the guest workload, stage the firmware artifacts, then run
the launcher:

```sh
make PYTHON='uv run --with cbor2' generate-keys
make guests
make PYTHON='uv run --with cbor2' PLATFORM=generic firmware
make -C test/standalone-tvm-launcher run
```

The last command starts QEMU. It prints the TVM attestation evidence and finishes with
`[HOST] Program completed. Halting`; stop QEMU with <kbd>Ctrl</kbd>+<kbd>C</kbd> afterwards.
`firmware` deliberately stages the TSM, firmware, signature, DICE input, and
`bin/generic/device-tree.dtb` before the launcher is built. Test launchers consume those artifacts
instead of rebuilding them implicitly.

## Repository structure

- `shadowfax/`: M-mode firmware, static domain setup, and the OpenSBI submodule.
- `tsm/`: trusted security monitor and TVM lifecycle implementation.
- `common/`: SBI definitions and attestation structures shared by firmware and TSM.
- `guests/bare-metal/`: freestanding TVM workloads, including the attestation guest.
- `guests/linux/`: Linux TVM kernel, BusyBox, and device-tree source configurations.
- `test/`: standalone launchers, functional tests, GDB scripts, and security scenarios.
- `scripts/`: host setup, DICE tooling, and the Linux/QEMU launcher.
- `bin/` and `target/`: generated firmware, signatures, payloads, and build artifacts.

## Architecture

The default platform configures three static OpenSBI domains:

- **Root domain:** owns resources not assigned to another domain and is not used as a workload.
- **Untrusted domain:** runs the host OS, VMM, or bare-metal CoVE-H client.
- **Trusted domain:** runs the TSM and owns confidential TVM memory.

The implementation currently covers parts of SUPD, CoVE-H, and CoVE-G. CoVE-I and hardware-assisted
interrupt virtualization are not implemented.

Use `make build-info` to check the detected toolchain and platform. Pass `RV_PREFIX` explicitly if
the RISC-V tools are not available under the default prefix. Shared compiler, assembler, linker,
architecture, and QEMU defaults live in `config.mk`; see [SETUP.md](SETUP.md#shared-make-configuration)
for supported overrides and debug behavior.

## How the attestation example works

The most complete standalone demonstration runs a bare-metal CoVE host, creates a bare-metal TVM,
and retrieves the layered attestation evidence containing the platform certificate:

```sh
make -C test/standalone-tvm-launcher/ run
```

The standalone launcher embeds the selected guest and creates the bare-metal host image. When
`run` starts QEMU:

1. The complete `guests/bare-metal/attestation.out` ELF is embedded in the host executable's
   `.guest_elf` section.
2. QEMU loads the existing firmware, device tree, DICE input, and bare-metal host.
3. The host uses SUPD to discover the TSM, then CoVE-H calls to donate confidential pages, create
   the TVM, map measured ELF segments, create a vCPU, finalize the measurement, and enter the TVM.
4. The guest invokes CoVE-G `GET_EVIDENCE`; the TSM returns the platform, TSM, and TVM evidence,
   which the guest prints to the QEMU console.

Use `GUEST_ELF=/path/to/guest.out` to embed another RISC-V ELF or `DTB=/path/to/tree.dtb` when
running with another prebuilt device tree. Missing staged inputs report the command needed to build
them instead of being built implicitly.

## Boot Linux as a TVM guest

The standalone TSM can boot a Linux kernel as a confidential VS-mode TVM.
This path consumes an ELF kernel rather than a raw `Image`, and embeds a TVM-specific DTB and initramfs:

```text
linux/guest/vmlinux
bin/linux-tvm.dtb
bin/linux-tvm-initramfs.cpio.gz
```

Build those artifacts using the committed configurations and instructions in
[`guests/linux/README.md`](guests/linux/README.md), then run:

```sh
make -B tsm CARGO_FLAGS="--features standalone"
qemu-system-riscv64 -M virt -nographic -smp 1 -m 1G \
    -kernel target/riscv64imac-unknown-none-elf/debug/tsm
```

The `standalone` feature is needed because, normally, the TSM behaves like a `trap-handler`
cooperating with the untrusted domain through the firmware.

This is intentionally separate from the Linux host workflow below. The Linux TVM guest runs inside
the TSM's confidential 256 MiB guest-physical address space; the Linux host runs as Shadowfax's
untrusted supervisor domain.

## Boot Linux as the untrusted host

Prepare these local artifacts as described in [SETUP.md](SETUP.md#linux-untrusted-host):

- `linux/host/arch/riscv/boot/Image`
- `bin/linux-host-initramfs.cpio.gz`

Then run:

```sh
./scripts/run-linux.sh
```

The script validates the kernel, initramfs, and DTB address ranges, rebuilds the firmware, starts
QEMU user networking, obtains the guest address through DHCP, and forwards host port 2222 to SSH:

```sh
ssh -p 2222 root@127.0.0.1
```

## Common commands

```sh
make help                                      # list supported targets
make build-info                                # show detected build settings
make PYTHON='uv run --with cbor2' firmware      # stage firmware and the platform DTB
make PYTHON='uv run --with cbor2' test          # run the QEMU boot integration test
make PYTHON='uv run --with cbor2' qemu-run      # boot the firmware directly
```

See [DEBUG.md](DEBUG.md) for GDB-driven CoVE scenarios.

## Contributing and references

Keep changes focused and run `make test` before submitting firmware modifications. Install the
repository's pre-commit hooks when preparing a contribution. Keep unsafe code small and document
hardware, address-layout, and SBI assumptions near the implementation.

Shadowfax builds on the RISC-V
[AP-TEE specification](https://github.com/riscv-non-isa/riscv-ap-tee),
[OpenSBI](https://github.com/riscv-software-src/opensbi), and selected H-CSR code from
[Hikami](https://github.com/Alignof/hikami). RV8 and RISC-V test workloads remain in their
respective vendored directories.
