# shadowfax

> [!WARNING]
> `shadowfax` is an early development project.

The codename `shadowfax project` aims to establish the foundation for an open-source software ecosystem for
confidential computing on RISC-V, similar to ARM TrustFirmware. The current RISC-V standard for confidential
computing is defined in the RISC-V AP-TEE specification, also known as CoVE
(**Co**nfidential **V**irtualization **E**xtension).

Further details can be found in the [documentation](https://granp4sso.github.io/shadowfax/).

### Goals
The codename `shadowfax project` has the following goals:
- Develop an open-source TSM-Driver that runs alongside OpenSBI.
- Implement the core functionalities of the CoVE SBI specification.
- Enable Supervisor Domain management using the MPT if available, or the PMP as a fallback.
- Write the implementation in a memory-safe language (e.g., Rust).

### OpenSBI integration
Shadowfax is an *M-mode* firmware which uses [**opensbi**](https://github.com/riscv-software-src/opensbi) as
static library. Shadowfax registers 3 SBI extensions described in the [CoVE specification](https://github.com/riscv-non-isa/riscv-ap-tee)
which are:

- SUPD: supervisor doamin extension to enumerate active supervisor domain and get capabilities information on them;
- CoVE-H: cove host extension. It allows for **TVM** management for hosts;
- CoVE-G: cove guest extension. It allows guest to use firmware services like remote attestation primitives;

The CoVE specification also introduces the **CoVE-I** SBI extension. It allows to supplements CoVE-H with hardware-assisted
interrupt virtualization using RISC-V **Advanced Interrupt Architecture**(*AIA*), if the platform supports it.
For now, shadowfax **does not** implement this part of the specification.

## Environment setup

Shadowfax generates automatically opensbi bindings using `bindgen` API in `build.rs`.


> [!NOTE]
> if you are building on a **musl** system make sure to check out the [building on musl systems](#building-on-musl-systems).

The `scripts` directory contains utilities to help setup the shadowafax build environment. It also contains scripts
that help building and running examples (ie. bare metal *S-mode* kernel or to launch Linux). More information [here](/scripts/README.md).

All dependencies can be installed with the `scripts/setup.sh` script.

```sh
sudo ./scripts/setup.sh
```
After the installation, configure your shell using `source scripts/settings.sh` (this will setup
the current shell variables like **CROSS_COMPILE**) and run the helloworld to check if the setup is
working:

```sh
cargo run
```

### Builing on musl systems
Musl is a security and safety oriented libc implementation which requires static linking. Building on
musl needs more setup because `bindgen` has a direct depenndency with `libclang` and most Linux distribution
do not ship `libclang.a`, so during the setup phase (this is handled by `scripts/setup.sh`), `shadowfax`
will attempt to build `libclang.a` from source (requires some time). `Cargo.toml` will be modified removing
the following:

```toml
[build-dependencies]
bindgen = "0.71.1"
```
And adding the `bindgen` and `clang` crate with the *static* feature enabled.

```toml
[build-dependencies]
bindgen = { version = "0.71.1", default-features = false, features = ["logging", "prettyplease", "static"] }

[build-dependencies.clang-sys]
version = "1.8.1"
features = ["static"]
```

> [!TIP]
> everything related to `build-dependencies` and `build.rs` affect the host building system and not the `ŧarget` itself.

The `scripts/environment.sh` will setup extra clang variables to point to the new built `libclang`:
```sh
export LIBCLANG_STATIC=1
export LIBCLANG_PATH=$(pwd)/llvm-project-${LLVM_VERSION}.src/build/lib
export LIBCLANG_STATIC_PATH=$(pwd)/llvm-project-${LLVM_VERSION}.src/build/lib
export LLVM_CONFIG_PATH=$(pwd)/scripts/llvm-config.sh
```

Due to some bugs in [`clang-sys`](https://github.com/KyleMayes/clang-sys?tab=readme-ov-file#environment-variables), the `scripts/llvm-config.sh` is needed as a workaround as described [here](https://github.com/rust-lang/rust-bindgen/issues/2360).

Also, users will need to change the linker in `.cargo/config.toml` from `riscv64-linux-gnu-ld` to `riscv64-linux-musl-ld`.

### Unsupported distributions
If your distribution is not supported by the script, you can install required dependencies by yourself or refer to the [Docker setup](#docker-setup). You need:

- a riscv64 toolchain: to compile source code and examples;
- qemu (for riscv64): to run programs in an emulated machine;
- dependencies to build the Linux Kernel;
- rust with the riscv64imac target;

### Docker setup
For unsupported distributions or for users that want a consistent build environment,
a debian-based Docker image can be built and executed in container with:
using `scripts/Dockerfile.setup`:
```sh
docker build -t shadowfax-build \
    --build-arg USER_ID=$(id -u) \
    --build-arg PLATFORM=generic \
    --build-arg OPENSBI=1.6 \
    --file scripts/Dockerfile.setup .
docker run -v $(pwd):/shadowfax -w /shadowfax --network=host -it shadowfax-build
```

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
