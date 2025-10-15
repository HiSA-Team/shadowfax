# shadowfax

> [!WARNING]
> `shadowfax` is an early development project.

The codename `shadowfax project` aims to establish the foundation for an open-source software ecosystem for
confidential computing on RISC-V, similar to ARM TrustFirmware. The current RISC-V standard for confidential
computing is defined in the RISC-V AP-TEE specification, also known as CoVE
(**Co**nfidential **V**irtualization **E**xtension).

This code is tested on `riscv64imac` with Privilege ISA **v1.12** with OpenSBI **v1.6**.

Further details can be found in the [documentation](https://granp4sso.github.io/shadowfax/).

### Goals
The codename `shadowfax project` has the following goals:
- Develop an open-source TSM-Driver that runs alongside OpenSBI.
- Implement the core functionalities of the CoVE SBI specification.
- Enable Supervisor Domain management using the MPT if available, or the PMP as a fallback.
- Write the implementation in a memory-safe language (e.g., Rust).

### OpenSBI integration
Shadowfax is an *M-mode* firmware which uses [**OpenSBI**](https://github.com/riscv-software-src/opensbi)
as static library. OpenSBI is included as a _git submodule_ in `shadowfax/opensbi` and it will be
built together with the firmware using `shadowfax/build.rs` script. Thus, users will need to clone:

```sh
git clone --recursive https://github.com/HiSA-Team/shadowfax
```

Shadowfax registers 2 SBI extensions described in the [CoVE specification](https://github.com/riscv-non-isa/riscv-ap-tee)
which are:

- SUPD: supervisor doamin extension to enumerate active supervisor domain and get capabilities information on them;
- CoVE-H: cove host extension. It allows **TVM** management for hosts;

The CoVE specification also introduces the **CoVE-I** SBI extension. It allows to supplements CoVE-H with hardware-assisted
interrupt virtualization using RISC-V **Advanced Interrupt Architecture**(*AIA*), if the platform supports it.
For now, shadowfax **does not** implement this part of the specification.

## Environment setup

To export relevant environment variables, users will need to source the `environment.sh` file specifing
an OpenSBI path. This script does not install anything but configures the current shell with correct
settings for platform detection.

```
source environment.sh
```

### Dependency installation

Shadowfax generates automatically OpenSBI bindings using `bindgen` API in `build.rs`.

The `scripts` directory contains utilities to help setup the shadowafax build environment.
More information [here](/scripts/README.md).

All dependencies can be installed with the `scripts/setup.sh` script.

```sh
sudo ./scripts/setup.sh
```

> [!TIP]
> everything related to `build-dependencies` and `build.rs` affect the host building system and not the `ŧarget` itself.

The `environment.sh` will setup extra clang variables to point to the new built `libclang`:
```sh
export LIBCLANG_STATIC=1
export LIBCLANG_PATH=$(pwd)/llvm-project-${LLVM_VERSION}.src/build/lib
export LIBCLANG_STATIC_PATH=$(pwd)/llvm-project-${LLVM_VERSION}.src/build/lib
export LLVM_CONFIG_PATH=$(pwd)/scripts/llvm-config.sh
```

Due to some bugs in [`clang-sys`](https://github.com/KyleMayes/clang-sys?tab=readme-ov-file#environment-variables), the `scripts/llvm-config.sh` is needed as a workaround as described [here](https://github.com/rust-lang/rust-bindgen/issues/2360).

### Unsupported distributions
If your distribution is not supported by the script, you can install required dependencies by
yourself or refer to the [Docker setup](#docker-setup). You need:

- a riscv64 toolchain: to compile source code and examples;
- qemu (for riscv64): to run programs in an emulated machine;
- dependencies to build the Linux Kernel;
- rust with the riscv64imac target;

### Docker and devcontainer setup
For unsupported distributions or for users that want a consistent build environment,
a debian-based Docker image can be built and executed in container using `Dockerfile`:

```sh
docker build -t shadowfax-build \
    --build-arg USER_ID=$(id -u) \
    --build-arg PLATFORM=generic \
docker run -v $(pwd):/shadowfax -w /shadowfax -it shadowfax-build
```

If using modern editors like VS-code, the repository supports [devcontainer workspaces](https://containers.dev/) and should automatically
ask you to create a new workspace when creating using the `.devcontainer/devcontainer.json` file.

## Running on QEMU
Users can run the firmware on QEMU using:

```sh
qemu-system-riscv64 -monitor unix:/tmp/shadowfax-qemu-monitor,server,nowait -nographic \
    -M virt -m 64M -smp 1 \
    -dtb bin/device-tree.dtb \
    -bios target/riscv64imac-unknown-none-elf/debug/shadowfax \
    -s -S
```

This will stop the emulator on the first instruction. You can setup a basic teecall/teeret example
in another terminal with a remote gdb session. For example, to test a basic program that calls
`sbi_covh_get_tsm_info` function run:

```sh
gdb -x scripts/gdb_settings -x scripts/sbi_covh_get_tsm_info.py

# step through multiple breakpoints
(gdb) continue
```

A more complicated example with a _synthetic_ TVM can be executed by running:
```sh
gdb -x scripts/gdb_settings -x scripts/sbi_covh_create_tvm.py

# step through multiple breakpoints
(gdb) continue
```

The `sbi_covh_create_tvm.py` script will perform the following action simulating the creation of a
trusted virtual machine which will be performed by a CoVE-aware (untrusted) OS/Hypervisor:

- perform supervisor domain enumeration (discovers the TSM)
- check TSM capabilities
- donate some memory to the TSM (which will become confidential memory)
- create the TVM objects
- add TVM memory region
- copy the src code of the TVM in the donated region
- create the TVM vCPU
- run the TVM vCPU

The TVM code is just an infinite loop for demonstration purposes.

## Contributing
This repository uses [pre-commit](https://pre-commit.com/). Before contributing, setup your environment
with the correct hooks. Create a virtual environment for Python using `.python-version` file.
For example:

```sh
python -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
pre-commit install
```

If you have `uv` you can use the [tool API](https://docs.astral.sh/uv/concepts/tools/#the-bin-directory).
```sh
uv tool install pre-commit
```
