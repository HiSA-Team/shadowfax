# Shadowfax

> [!WARNING]
> Shadowfax is an early-stage research project. Interfaces and memory layouts may change.

Shadowfax is an open-source RISC-V confidential-computing firmware stack based on the
Application-TEE (CoVE) specification. It combines an M-mode TSM driver, OpenSBI, and a trusted
security monitor (TSM) capable of creating and running trusted virtual machines (TVMs).

The project is tested with the `riscv64imac` target, RISC-V Privileged ISA v1.12, OpenSBI v1.7,
and QEMU's `virt` machine.

## Documentation index

- [Quick setup](#quick-setup): clone the project, generate keys, and verify the toolchain.
- [Bare-metal TVM attestation](#the-holy-grail-bare-metal-host-and-tvm-attestation): run the
  complete host-to-TVM demonstration.
- [Linux host](#boot-linux-as-the-untrusted-host): boot Linux with an initramfs and SSH forwarding.
- [SETUP.md](SETUP.md): detailed dependencies, toolchains, keys, Linux, initramfs, Docker, and musl
  instructions.
- [DEBUG.md](DEBUG.md): QEMU/GDB startup, synthetic CoVE-H scenarios, and debugger commands.

## Repository structure

- `shadowfax/`: M-mode firmware, static domain setup, and the OpenSBI submodule.
- `tsm/`: trusted security monitor and TVM lifecycle implementation.
- `common/`: SBI definitions and attestation structures shared by firmware and TSM.
- `guests/`: bare-metal TVM workloads, including the attestation guest.
- `test/`: standalone launchers, functional tests, GDB scripts, and security scenarios.
- `scripts/`: host setup, DICE tooling, and the Linux/QEMU launcher.
- `docs/source/`: Sphinx reference documentation.
- `bin/` and `target/`: generated firmware, signatures, payloads, and build artifacts.

## Architecture

Shadowfax configures three static OpenSBI domains:

- **Root domain:** owns resources not assigned to another domain and is not used as a workload.
- **Untrusted domain:** runs the host OS, VMM, or bare-metal CoVE-H client.
- **Trusted domain:** runs the TSM and owns confidential TVM memory.

The implementation currently covers parts of SUPD, CoVE-H, and CoVE-G. CoVE-I and hardware-assisted
interrupt virtualization are not implemented.

## Quick setup

Clone the submodules, install the dependencies described in [SETUP.md](SETUP.md), and generate the
local development keys:

```sh
git clone --recurse-submodules https://github.com/HiSA-Team/shadowfax
cd shadowfax
make generate-keys PYTHON='uv run --with cbor2'
make -B PYTHON='uv run --with cbor2'
```

Use `make build-info` to check the detected toolchain and platform. Pass `RV_PREFIX` explicitly if
the RISC-V tools are not available under the default prefix. Shared compiler, assembler, linker,
architecture, and QEMU defaults live in `config.mk`; see [SETUP.md](SETUP.md#shared-make-configuration)
for supported overrides and debug behavior.

## The holy grail: bare-metal host and TVM attestation

The most complete standalone demonstration runs a bare-metal CoVE host, creates a bare-metal TVM,
and retrieves the layered attestation evidence containing the platform certificate:

```sh
make -C test/standalone-tvm-launcher/ run
```

The root build prepares Shadowfax, the TSM, its signature, the default attestation guest, and the
DICE-derived platform attestation input. The standalone launcher Makefile itself only embeds the
selected guest and creates the bare-metal host image. When `run` starts QEMU:

1. The complete `guests/attestation.out` ELF is embedded in the host executable's `.guest_elf`
   section.
2. QEMU loads the existing firmware, device tree, DICE input, and bare-metal host.
3. The host uses SUPD to discover the TSM, then CoVE-H calls to donate confidential pages, create
   the TVM, map measured ELF segments, create a vCPU, finalize the measurement, and enter the TVM.
4. The guest invokes CoVE-G `GET_EVIDENCE`; the TSM returns the platform, TSM, and TVM evidence,
   which the guest prints to the QEMU console.

Use `GUEST_ELF=/path/to/guest.out` to embed another RISC-V ELF or `DTB=/path/to/tree.dtb` when
running with another prebuilt device tree. Missing inputs are reported instead of being built
implicitly.

## Boot Linux as the untrusted host

Prepare these local artifacts as described in [SETUP.md](SETUP.md#linux-host):

- `linux/host/arch/riscv/boot/Image`
- `bin/initramfs.cpio.gz`

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
make -B PYTHON='uv run --with cbor2'           # force a complete measured build
make test PYTHON='uv run --with cbor2'         # run the QEMU boot integration test
make qemu-run PYTHON='uv run --with cbor2'     # boot the firmware directly
```

See [DEBUG.md](DEBUG.md) for GDB-driven CoVE scenarios.

## Contributing and references

Keep changes focused and run `make test` before submitting firmware modifications. Install the
repository's pre-commit hooks when preparing a contribution. Keep unsafe code small and document
hardware, address-layout, and SBI assumptions near the implementation.

Shadowfax builds on the RISC-V
[AP-TEE specification](https://github.com/riscv-non-isa/riscv-ap-tee),
[OpenSBI](https://github.com/riscv-software-src/opensbi), and selected H-CSR code from
[Hikami](https://github.com/Alignof/hikami). CoreMark and RISC-V test workloads remain in their
respective vendored directories.
