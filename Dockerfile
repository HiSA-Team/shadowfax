# The goal of this Dockerfile is to create a consistent build environment.
# The base image is rust:bookworm which is based on Debian 12.
#
# Project dependencies are:
# - rust toolchain
# - gcc riscv toolchain
# - qemu
#
# Usage: run this image in a container mounting the project directory as a bind volume.
#   docker build -t shadowfax-build --build-arg USER_ID=$(id -u) .
#
# Default starts a shell environment:
#   docker run -v $(pwd):/shadowfax -it shadowfax-build
#
# Execute an example with:
#   docker run --rm -v $(pwd):/shadowfax -it shadowfax-build bash -c "make build-info"
#
# Author: Giuseppe Capsso <capassog97@gmail.com>

FROM rust:1-bookworm

# Build args for UID matching and other options
ARG USER_ID=1000

# Install required dependencies
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install --no-install-recommends \
      libssl-dev qemu-system-riscv64 curl build-essential make ca-certificates git libglib2.0-dev \
      libfdt-dev libpixman-1-dev zlib1g-dev ninja-build autoconf automake autotools-dev curl python3 \
      python3-pip python3-tomli libmpc-dev libmpfr-dev libgmp-dev gawk build-essential bison flex \
      texinfo gperf libtool patchutils bc zlib1g-dev libexpat-dev ninja-build git cmake libglib2.0-dev \
      libslirp-dev sudo device-tree-compiler libclang-dev \
    && rm -rf /var/lib/apt/lists/*

# Download and build QEMU
RUN curl -fsSL https://download.qemu.org/qemu-10.1.1.tar.xz -o /tmp/qemu-10.1.1.tar.xz \
    && tar xvJf /tmp/qemu-10.1.1.tar.xz -C /tmp/

WORKDIR /tmp/qemu-10.1.1
RUN ./configure --target-list=riscv64-softmmu \
    && make -j $(nproc) && make install

# Download and build GCC-RISC-V toolchain
RUN git clone https://github.com/riscv/riscv-gnu-toolchain /tmp/riscv-gnu-toolchain
WORKDIR /tmp/riscv-gnu-toolchain
RUN git checkout 2025.10.28 \
    && ./configure --prefix=/opt/riscv --with-abi=lp64 --with-languages=c \
    && make linux -j $(nproc)

# Create non-root user matching host UID
RUN useradd -m -u ${USER_ID} -s /bin/bash devuser \
    && usermod -aG sudo devuser \
    && echo '%sudo ALL=(ALL) NOPASSWD:ALL' >> /etc/sudoers

# Workdir for project
WORKDIR /shadowfax
RUN chown -R devuser:devuser /shadowfax

USER devuser

# Install toolchain from rust-toolchain.toml
COPY --chown=devuser:devuser rust-toolchain.toml .
RUN rustup show

ENV PATH="$PATH:/opt/riscv/bin"
