# Common settings for the bare-metal builds.

DEBUG         ?= 0
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
LDFLAGS = -march=$(ARCH) -mabi=$(ABI) -mcmodel=medany \
	-nostdlib -nostartfiles -static -no-pie

CONFIG_DIR      := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
GUEST_DIR       ?= $(CONFIG_DIR)guests/bare-metal
GUEST_BUILD_DIR ?= $(GUEST_DIR)/build
GUEST_CFLAGS    ?= -I$(GUEST_DIR)/include
GUEST_ASFLAGS   ?= -I$(GUEST_DIR)
GUEST_LIB       ?= $(GUEST_BUILD_DIR)/libbaremetal.a
GUEST_LDFLAGS   ?= $(GUEST_LIB)
GUEST_MEMORY_SIZE ?= 4194304

ifeq ($(DEBUG),1)
CFLAGS  += -O0 -g
ASFLAGS += -g
LDFLAGS += -g
else
CFLAGS  += -O1
endif

QEMU       ?= qemu-system-riscv64
QEMU_FLAGS  = -M virt -m 512M -smp 1 -nographic
ifeq ($(DEBUG),1)
QEMU_FLAGS += -s -S -monitor unix:/tmp/shadowfax-qemu-monitor,server,nowait
endif
