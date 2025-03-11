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

## Environment setup
All dependencies can be installed with the `scripts/setup.sh` script. It automatically detects your distribution using
`lsb_release -c` and installs dependencies accordingly. Usually, `lsb_release` is available by default on most systems.
If the script outputs something like:
```
./scripts/setup.sh:18: lsb_release command not found
```
You must install `lsb_release` first. For example on Ubuntu:
```sh
sudo apt-get install lsb_release
```
Now, you can run the script with sudo:
```sh
sudo ./scripts/setup.sh
```
After the installation, you can check if the setup is working with:

```sh
make -C examples/helloworld CROSS_COMPILE=riscv64-linux-gnu- run
```
On success, you should see the following output:
```
Press (ctrl + a) and then x to quit
qemu-system-riscv64 -nographic -machine virt -bios main
shadowfax says: 5 + 4 = 9
```

### Unsupported distributions
If your distribution is not supported by the script, you can install required dependencies
by yourself. You need:

- a riscv64 toolchain: to compile source code and examples;
- qemu (for riscv64): to run programs in an emulated machine;
- make: to assemble projects;
- rust toolchain: refer to https://rustup.rs/. Install the `riscv64gc-unknown-none-elf` target
    with `rustup target add riscv64gc-unknown-none-elf`

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
