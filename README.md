# shadowfax

The codename `shadowfax project` aims to establish the foundation for an open-source software ecosystem for
confidential computing on RISC-V, similar to ARM TrustFirmware. The current RISC-V standard for confidential
computing is defined in the RISC-V AP-TEE specification, also known as CoVE
(**Co**nfidential **V**irtualization **E**xtension).

Further details can be found in the documentation.

### Goals
The codename `shadowfax project` has the following goals:
- Develop an open-source TSM-Driver that runs alongside OpenSBI.
- Implement the core functionalities of the CoVE SBI specification.
- Enable Supervisor Domain management using the MPT if available, or the PMP as a fallback.
- Write the implementation in a memory-safe language (e.g., Rust).

### OpenSBI integration
Shadowfax is an *M-mode* firmware which uses [**opensbi**](https://github.com/riscv-software-src/opensbi) as
static library. Shadowfax registers 4 SBI extensions described in the [CoVE specification](https://github.com/riscv-non-isa/riscv-ap-tee)
which are:

- SUPD: supervisor doamin extension to enumerate active supervisor domain and get capabilities information on them;
- CoVE-H: cove host extension. It allows for **TVM** management for hosts;
- CoVE-G: cove guest extension. It allows guest to use firmware services like remote attestation primitives;
- CoVE-I: cove interrupt extension. It allows to supplements CoVE-H with hardware-assisted interrupt virtualization
    using RISC-V Advanced Interrupt Architecture (AIA), if the platform supports it;

## Environment setup
All dependencies can be installed with the `scripts/setup.sh` script.

```sh
sudo ./scripts/setup.sh
```
After the installation, configure your shell using `source scripts/settings.sh` (this will setup
the current shell variables like **CROSS_COMPILE**) and run the helloworld to check if the setup is
working:

```sh
make -C examples/helloword run
```
On success, you should see the following output:
```
Press (ctrl + a) and then x to quit
qemu-system-riscv64 -nographic -machine virt -bios main
shadowfax says: 5 + 4 = 9
```

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
