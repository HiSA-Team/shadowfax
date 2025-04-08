# Shadowfax scripts

***NOTE*** It is assumed that scripts are executed from the root project directory.

This folder contains utilities which are used to setup and automate some process for Shadowfax development which are:

- **environment.sh**: a script which detects and configures the current host. For example, the host architecture, the libc implementation and sets variables according this parameters to build shadowafax binary. This script is meant to be **sourced** and **not executed**. For example, `source scripts/environment.sh`;

- **setup.sh**: this scripts must be run as superuser and it install correct dependencies to build and run shadowafax. This script detects the current distributions and install dependencies accordingly. Major `debian-based` distribution are supported (managed with *apt*). Next, it installs rust toolchain if not present. Finally, it builds and installs **opensbi**. The output of opensbi installation is:
    * **include/**: an include directory for all type definitions. This folder is targeted by `build.rs` which used *bindgen* to generate rust types;
    * **lib64/**: this directory contains the static library which will be linked against shadowfax binary;
In case of *musl* systems, the `setup.sh` attempts to build *clang* from scratch because *musl* does not support dynamic linking. This is due to the fact that *bindgen* requires libclang and most Linux distribution do not package `libclang.a`.

- **Dockerfile.setup**: a Dockerfile which creates a reproducibile build environment based on *debian-12* similarly to what it is done in `setup.sh`;


## Quick start
The typical development workflow is:

- run `setup.sh`: this correctly installs all the needed dependencies to work with **shadowafax**;
- source `environment.sh`: this ensures all build variables are correctly configured before building/running shadowafax;
- build and run shadowafax: shadowfax is an *M-mode* firmware and needs an *S-mode* payload to execute real programs. If an *S-mode* payload is not provided a *stub* payload will be used for test purpose;

## Docker setup
`shadowafax` can be developed/built using *Docker*. An image can be built with the command:

```sh
docker build -t shadowfax-build \
    --build-arg USER_ID=$(id -u) \
    --build-arg PLATFORM=generic \
    --build-arg OPENSBI=1.6 \
    --file scripts/Dockerfile.setup .
```

It can be used to open an interactive shell (which already sources `scripts/environment.sh`):
```sh
docker run -v $(pwd):/shadowfax -it shadowfax-build
```

Or to run a command (already sourcing `scripts/environment.sh`):
```sh
docker run --rm -v $(pwd):/shadowfax -it shadowfax-build make -C examples/helloworld-c run
```

For development purposes, users can make use of [`devcontainers`](https://containers.dev/) and integrated the build process into their IDE (through the `.devcontainer` directory).
