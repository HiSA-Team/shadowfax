# The goal of this Dockerfile is to create a consistent build environment.
# The base image is rust:bookworm which is based on Debian 12.
# Project dependencies are:
# - rust toolchain (with riscv64imac target)
# - gcc riscv toolchain
# - qemu
#
# Usage: run this image in a container mounting the project directory as a bind volume.
#
#   docker build -t shadowfax-build \
#     --build-arg USER_ID=$(id -u) \
#     --build-arg PLATFORM=generic
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
ARG PLATFORM=generic

# Install system dependencies for RISC-V dev
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install --no-install-recommends \
    autoconf automake autotools-dev bc bison bsdextrautils build-essential cmake curl \
    device-tree-compiler flex gawk gcc-riscv64-linux-gnu git gperf libclang-dev libelf-dev \
    libexpat-dev libgmp-dev libmpc-dev libmpfr-dev libglib2.0-dev libslirp-dev libssl-dev libtool \
    make patchutils python3-venv python3-tomli ninja-build sudo texinfo zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

# Download and build QEMU
RUN curl -fsSL https://download.qemu.org/qemu-10.1.1.tar.xz -o /tmp/qemu-10.1.1.tar.xz \
    && tar xvJf /tmp/qemu-10.1.1.tar.xz -C /tmp/

WORKDIR /tmp/qemu-10.1.1
RUN ./configure --target-list=riscv64-softmmu \
    && make -j $(nproc) && make install

# Create non-root user matching host UID
RUN useradd -m -u ${USER_ID} -s /bin/bash devuser \
    && usermod -aG sudo devuser \
    && echo '%sudo ALL=(ALL) NOPASSWD:ALL' >> /etc/sudoers

# Copy environment setup
COPY environment.sh /environment.sh

# Workdir for project
WORKDIR /shadowfax
RUN chown -R devuser:devuser /shadowfax

USER devuser

# Install toolchain from rust-toolchain.toml
COPY --chown=devuser:devuser rust-toolchain.toml .
RUN rustup show

USER root
RUN echo '#!/bin/sh' > /entrypoint.sh \
    && echo ". /environment.sh" >> /entrypoint.sh \
    && echo 'exec "$@"' >> /entrypoint.sh
RUN cp /entrypoint.sh /etc/profile.d/shadowfax.sh
RUN chmod +x /entrypoint.sh
USER devuser

ENTRYPOINT ["/entrypoint.sh"]
CMD ["bash"]
