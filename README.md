# shadowfax

> [!WARNING]
> `shadowfax` is an early development project.

The codename `shadowfax project` aims to establish the foundation for an open-source software ecosystem for
confidential computing on RISC-V, similar to ARM TrustFirmware. The current RISC-V standard for confidential
computing is defined in the RISC-V AP-TEE specification, also known as CoVE
(**Co**nfidential **V**irtualization **E**xtension).

This code is tested on `riscv64imac` with Privilege ISA **v1.12** with OpenSBI **v1.7**.

The repository has the following layout:
- [**tsm**](tsm/): contains all the TSM and trusted hypervisor code;
- [**shadowfax**](shadowfax/): contains all data for the TSM-driver including OpenSBI firmware;
- [**benchmark**](benchmark/): benchmark results and a script to process and visualize results with [**marimo**](https://marimo.io/);
- [**test**](test/): contains test material;

### Goals
The codename `shadowfax project` has the following goals:
- Develop an open-source TSM-Driver that runs alongside OpenSBI.
- Implement the core functionalities of the CoVE SBI specification.
- Enable Supervisor Domain management using the PMP (switch to MPT if available).
- Write the implementation in a memory-safe language (e.g., Rust).

### OpenSBI integration
Shadowfax is an *M-mode* firmware which uses [**OpenSBI**](https://github.com/riscv-software-src/opensbi)
as static library. OpenSBI is included as a _git submodule_ in `shadowfax/opensbi` and it will be
built together with the firmware using `shadowfax/build.rs` script. Thus, users will need to clone:

```sh
git clone --recurse-submodules https://github.com/HiSA-Team/shadowfax
```

Shadowfax implements (partially) 3 SBI extensions described in the [CoVE specification](https://github.com/riscv-non-isa/riscv-ap-tee)
which are:

- SUPD: supervisor doamin extension to enumerate active supervisor domain and get capabilities information on them;
- CoVE-H: cove host extension. It allows **TVM** management for hosts;
- CoVE-G: confidential features for Guests

The CoVE specification also introduces the **CoVE-I** SBI extension. It allows to supplements CoVE-H with hardware-assisted
interrupt virtualization using RISC-V **Advanced Interrupt Architecture**(*AIA*), if the platform supports it.
For now, shadowfax **does not** implement this part of the specification.

## Environment setup

Users will have to make sure that they have a working `riscv64` toolchain.
Users on Ubuntu 22.04 or 24.04 or Debian 12 can install their dependencies using the `setup.sh` script
by running:

```sh
sudo ./scripts/setup.sh
```

> [!TIP]
> everything related to `build-dependencies` and `build.rs` affect the host building system and not the `ŧarget` itself.

Configuring, building and running examples are performed through the single `Makefile`.

### Using a musl system as a host
If users have are on a musl system they will have to specify 2 extra environment variables pointing to
their `libclang.a`. This is required by the [`clang-sys`](https://github.com/KyleMayes/clang-sys?tab=readme-ov-file#static)
crate which is used to generate Opensbi bindings. Basically, users will have to build llvm from source in
order to provide `libclang.a`. After the build, users will have to provide:

- **LLVM_CONFIG_PATH**: pointing to their llvm-config binary in the build directory
- **LIBCLANG_STATIC_PATH**: pointing to the `lib` directory contains all static library built from LLVM.

As an example, users will have to do something like this:

```sh
git clone git@github.com:llvm/llvm-project.git
cd llvm-project

cmake -S llvm -B build -G Ninja -DLLVM_ENABLE_PROJECTS=clang -DLIBCLANG_BUILD_STATIC=ON
ninja -C build
```

### Unsupported distributions
If users are on a different distribution they will need to install required packages according to
their system. A list of complete dependencies can be obtained by looking at the list of installed
packages in the `scripts/setup.sh`:

```
$ apt-get install libssl-dev qemu-system-riscv64 curl build-essential make ca-certificates git
```

Otherwise they can leverage the provided `Dockerfile` to build a dedicated development environment.

> [!NOTE]
> The Docker image will build a QEMU (v10.1.1) and the riscv-toolchain (2025-10-28) from source.

```sh
# build the Docker image
docker build -t shadowfax-build --build-arg USER_ID=$(id -u)

# run a test container
docker run -v $(pwd):/shadowfax -w /shadowfax -it shadowfax-build sh -c "make build-info"
```

If using modern editors like VS-code, the repository supports [devcontainer workspaces](https://containers.dev/) and should automatically
ask to create a new workspace using the `.devcontainer/devcontainer.json` file.

## Building

The build process is managed through the `Makefile` in the root directory which will auto-detect
the host platform and settings.  To check the detected settings:

```sh
make build-info
```

First, users will need to generate ED25519 keypairs to sign the TSM:

```sh
make generate-keys
```

Finally, issue a full compilation:
```sh
make
```

Users may want to specify the following variables for their needs:
 - RV_PREFIX:           specify with the path to the target riscv toolchain prefix
 - BOOT_DOMAIN_ADDRESS: specify the address of the untrusted domain which should start the execution
 - PLATFORM:            target platform, this is used for OpenSBI initialization

## Running examples on QEMU
Users can run the firmware on QEMU using. This will make the TSM-driver spawn a test workload:

```sh
make qemu-run
```

### Test and debug
To test the full CoVE scenario, users can rely on the synthetic program generation to create an OS/VMM
emulation for simple programs. These programs are meant to be executed in GDB and run in a step by
step mode to inspect precise behaviour upon TEECALL/TEERET. Main programs are:

-  `sbi_covh_get_tsm_info`: gets the trusted hypervisor capabilities.
- `sbi_covh_create_tvm`: create a simple TVM that runs an endless loop.
- `sbi_covh_create_tvm_from_elf`: create a simple TVM from an ELF (maps each ELF segment in confidential
memory).

This will stop the emulator on start. Setup a basic TEECALL/TEERET example in another terminal with
a remote GDB session.
For example, to test a basic program that calls `sbi_covh_get_tsm_info` function:

```sh
make debug GDB_COVE_SCRIPT=test/debug/sbi_covh_get_tsm_info.py

# step through multiple breakpoints
(gdb) continue
```

A more complicated example with a _synthetic_ TVM can be executed by running:
```sh
make debug GDB_COVE_SCRIPT=test/debug/sbi_covh_create_tvm.py

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

## Reference projects
Rust H-CSR implementation has been taken from [Hikami](https://github.com/Alignof/hikami).
Coremark benchmark has been taken from [Coremeark](https://github.com/eembc/coremark).
RISC-V benchmarks have been taken from [Riscv-tests](https://github.com/riscv-software-src/riscv-tests).

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
