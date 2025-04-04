# Centralized file to manage build variables. This will be included in example, tests and scripts.
# Usage:
# 	When compiling use CROSS_COMPILE to pass the start of your ie. toolchain
# 	eg. make CROSS_COMPILE=riscv64-linux-gnu-
# Authors:
# 	Giuseppe Capasso <capassog97@gmail.com>

ifdef CROSS_COMPILE
CC		=	$(CROSS_COMPILE)gcc
AR		=	$(CROSS_COMPILE)ar
LD		=	$(CROSS_COMPILE)ld
AS		=	$(CROSS_COMPILE)as
else
CC		?=	gcc
AR		?=	ar
LD		?=	ld
AS		?=	as
endif

# Since only newer (version >=13) toolchains supports the 'H' extension of the ISA, we fallback to `gc` for older ones.
# This should not be a problem for an hypervisor/virtualization software.
# ARCH ?= $(shell if [ `$(CC) -dumpversion | cut -f1 -d.` -lt 13 ]; then echo rv64gc; else echo rv64gch; fi)
# Stick to rv64imac for now
ARCH = rv64imac
ABI = lp64

CFLAGS  = -Wall -Wextra -march=$(ARCH) -mabi=$(ABI)
LDFLAGS =
ASFLAGS = -march=$(ARCH) -mabi=$(ABI)

# Add debugging symbols if DEBUG=1
ifeq ($(DEBUG), 1)
	CFLAGS += -g
	LDFLAGS += -g
endif
