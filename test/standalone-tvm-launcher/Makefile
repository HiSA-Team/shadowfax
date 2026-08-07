# Build with `make`, run with `make run`, or embed another RISC-V ELF with
# `make GUEST_ELF=/path/to/guest.out`.

ROOT            := ../..
BUILD_DIR       ?= $(ROOT)/target/standalone-tvm-launcher
TARGET          ?= riscv64imac-unknown-none-elf
PROFILE         ?= debug
RV_PREFIX       ?= riscv64-unknown-elf-

CC              := $(RV_PREFIX)gcc
OBJCOPY         := $(RV_PREFIX)objcopy
QEMU            ?= qemu-system-riscv64

GUEST_ELF       ?= $(ROOT)/guests/attestation.out
FW_ELF          := $(ROOT)/target/$(TARGET)/$(PROFILE)/shadowfax
DICE_INPUT      := $(ROOT)/bin/shadowfax.dice.bin
DTB             := $(ROOT)/bin/device-tree.dtb
FDT_ADDR        := 0x8AF00000

ELF             := $(BUILD_DIR)/standalone-tvm-launcher
BIN             := $(ELF).bin
MAP             := $(ELF).map
OBJECTS         := $(BUILD_DIR)/startup.o $(BUILD_DIR)/main.o \
	$(BUILD_DIR)/guest_payload.o

ARCH_FLAGS      := -march=rv64imac -mabi=lp64 -mcmodel=medany
CFLAGS          := $(ARCH_FLAGS) -O2 -Wall -Wextra -Werror -ffreestanding \
	-fno-builtin -fno-pie -fno-stack-protector -msmall-data-limit=0
LDFLAGS         := $(ARCH_FLAGS) -nostdlib -nostartfiles -static -no-pie \
	-Wl,-T,linker.ld -Wl,--build-id=none -Wl,-Map,$(MAP)

.PHONY: all clean firmware run FORCE

all: $(ELF) $(BIN)

$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/main.o: main.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/startup.o: startup.S | $(BUILD_DIR)
	$(CC) $(ARCH_FLAGS) -c $< -o $@

$(ROOT)/guests/attestation.out:
	$(MAKE) -C $(ROOT)/guests attestation.out

$(BUILD_DIR)/guest_payload.o: $(GUEST_ELF) FORCE | $(BUILD_DIR)
	$(OBJCOPY) -I binary -O elf64-littleriscv -B riscv \
		--rename-section .data=.guest_elf,alloc,load,readonly,data,contents \
		$< $@

$(ELF): $(OBJECTS) linker.ld
	$(CC) $(LDFLAGS) $(OBJECTS) -o $@

$(BIN): $(ELF)
	$(OBJCOPY) -O binary $< $@

firmware:
	$(MAKE) -C $(ROOT) firmware

run: all firmware
	$(QEMU) -M virt -m 512M -smp 1 -nographic \
		-bios $(FW_ELF) \
		-device loader,file=$(DTB),addr=$(FDT_ADDR),force-raw=on \
		-device loader,file=$(DICE_INPUT),addr=0x88000000,force-raw=on \
		-device loader,file=$(BIN),addr=0x8A000000,force-raw=on

clean:
	$(RM) -r $(BUILD_DIR)

FORCE:
