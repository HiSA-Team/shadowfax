# The goal of this Dockerfile is to create a consistent build environment.
# The base image is rust:bullseye which is based on Debian 12.
# Project dependencies are:
# - rust toolchain (with riscv64imac target)
# - gcc riscv toolchain
# - shell utils to build Linux kernel
# - qemu
# - opensbi development files
#
# Usage: run this image in a container mounting the project directory as a bind
# volume. Opensbi is installed in /tmp/opensbi-{OPENSBI_VERSION} directory
#
#   docker build -t shadowfax-build \
#     --build-arg USER_ID=$(id -u) \
#     --build-arg PLATFORM=generic \
#     --build-arg OPENSBI=1.6 .
#
# Default starts a shell environment:
#   docker run -v $(pwd):/shadowfax -it shadowfax-build
#
# Execute an example with:
#   docker run --rm -v $(pwd):/shadowfax -it shadowfax-build bash -c "cargo build"
# Author: Giuseppe Capsso <capassog97@gmail.com>

FROM rust:1-bullseye

# Build args for UID matching and other options
ARG USER_ID=1000
ARG PLATFORM=generic
ARG OPENSBI_VERSION=1.6

# Install system dependencies for RISC-V dev
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y install --no-install-recommends \
    qemu-system-riscv64 gcc-riscv64-linux-gnu build-essential qemu-utils \
    libncurses-dev bison flex libssl-dev device-tree-compiler \
    libelf-dev dwarves curl git file cpio sudo bc libclang-dev gdb-multiarch \
    && rm -rf /var/lib/apt/lists/*

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
RUN rustup show \
    && rustup target add riscv64imac-unknown-none-elf \
    && rustup component add rust-analyzer

# Install OpenSBI
RUN curl -fsSL https://github.com/riscv-software-src/opensbi/archive/refs/tags/v$OPENSBI_VERSION.tar.gz -o /tmp/opensbi-$OPENSBI_VERSION.tar.gz \
    && tar xvf /tmp/opensbi-$OPENSBI_VERSION.tar.gz -C /tmp \
    && bash -c ". /environment.sh  /tmp/opensbi-${OPENSBI_VERSION} && make -C /tmp/opensbi-$OPENSBI_VERSION PLATFORM=$PLATFORM"

# Entrypoint
USER root
RUN echo '#!/bin/sh' > /entrypoint.sh \
    && echo ". /environment.sh /tmp/opensbi-${OPENSBI_VERSION}" >> /entrypoint.sh \
    && echo 'exec "$@"' >> /entrypoint.sh
RUN cp /entrypoint.sh /etc/profile.d/shadowfax.sh
RUN chmod +x /entrypoint.sh
USER devuser

ENTRYPOINT ["/entrypoint.sh"]
CMD ["bash"]
