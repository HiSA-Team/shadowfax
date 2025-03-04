# TODO: maybe use pkgconfig to find the correct path
PREFIX = riscv64-linux-musl

CC = $(PREFIX)-gcc
LD = $(PREFIX)-ld
AS = $(PREFIX)-as

ARCH = rv64gh

CFLAGS = -Wall -Wextra
LDFLAGS = 
