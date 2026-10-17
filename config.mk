# Common settings for the bare-metal builds.

DEBUG         ?= 0
PLATFORM      ?= generic
TARGET_TRIPLET ?= riscv64imac-unknown-none-elf
PROFILE       ?= debug

RV_PREFIX ?= riscv64-unknown-elf-
CC         = $(RV_PREFIX)gcc
AS         = $(RV_PREFIX)as
OBJCOPY    = $(RV_PREFIX)objcopy
GDB        = $(RV_PREFIX)gdb
AR         = $(RV_PREFIX)ar

ARCH ?= rv64imac
ABI  ?= lp64

CFLAGS  = -march=$(ARCH) -mabi=$(ABI) -mcmodel=medany -Wall -Wextra \
	-ffreestanding -fno-builtin -fno-pie -fno-stack-protector -msmall-data-limit=0
ASFLAGS = -march=$(ARCH) -mabi=$(ABI)
LDFLAGS = -march=$(ARCH) -mabi=$(ABI) -nostdlib -nostartfiles -static -no-pie

CONFIG_DIR      := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
BIN_DIR         ?= $(CONFIG_DIR)target/artifacts
LAYOUT_DIR      ?= $(CONFIG_DIR)target/platform/$(PLATFORM)
FDT_IMAGE       ?= $(LAYOUT_DIR)/device-tree.dtb
LAYOUT_MK       ?= $(LAYOUT_DIR)/layout.mk
LAYOUT_H_DIR    ?= $(LAYOUT_DIR)
LAYOUT_PY       ?= $(CONFIG_DIR)scripts/layout.py
GUEST_DIR       ?= $(CONFIG_DIR)guests/bare-metal
GUEST_BUILD_DIR ?= $(GUEST_DIR)/build
GUEST_CFLAGS    ?= -I$(GUEST_DIR)/include -I$(LAYOUT_H_DIR)
GUEST_ASFLAGS   ?= -I$(GUEST_DIR)
GUEST_LIB       ?= $(GUEST_BUILD_DIR)/libbaremetal.a
GUEST_LDFLAGS   ?= $(GUEST_LIB)
GUEST_RAM_SIZE    ?= 2097152

ifeq ($(DEBUG),1)
CFLAGS  += -O0 -g
ASFLAGS += -g
LDFLAGS += -g
else
CFLAGS  += -O1
endif

# Every Python helper in scripts/ runs through PYTHON3. The default is the
# plain interpreter, so the third-party modules in requirements.txt have to be
# importable from it: activate a virtualenv, or override the variable, e.g.
# PYTHON3='uv run --with cbor2 --with cryptography --with pycose --with pylibfdt python3'.
PYTHON3    ?= python3

QEMU       ?= qemu-system-riscv64
QEMU_FLAGS  = -M virt -m $(SHADOWFAX_QEMU_MEM) -smp $(SHADOWFAX_SMP) -nographic
ifeq ($(DEBUG),1)
QEMU_FLAGS += -s -S -monitor unix:/tmp/shadowfax-qemu-monitor,server,nowait
endif
