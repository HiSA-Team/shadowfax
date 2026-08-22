ROOT                := ../../..
LAUNCHER_NAME       := untrusted-supervisor-domain
BUILD_DIR           ?= $(ROOT)/target/security/$(LAUNCHER_NAME)
# QEMU's normal RISC-V boot ROM starts every hart at the beginning of a raw
# firmware image. Shadowfax therefore runs here as a conventional BIOS image,
# rather than through per-hart loader devices.
FIRMWARE            := $(ROOT)/bin/shadowfax.bin
GUEST_RAM_SIZE      := 16777216
GUEST_MEMORY_COUNT  := 1
PAYLOAD_SRC         := guest.c
DTB                 := $(BUILD_DIR)/device-tree.dtb
DTB_GENERATED       := 1
ATTACKER_ADDRESS    := 0x94000000
COORDINATION_ADDRESS := 0x96000000
ATTACKER_ELF        := $(BUILD_DIR)/attacker
ATTACKER_BIN        := $(ATTACKER_ELF).bin
RUN_QEMU_FLAGS      := -M virt -m 512M -smp 2 -nographic -monitor none -no-reboot \
	-device loader,file=$(abspath $(ATTACKER_BIN)),addr=$(ATTACKER_ADDRESS),force-raw=on

include $(ROOT)/guests/bare-metal/launcher.mk

GENERATED_DTS       := $(BUILD_DIR)/device-tree.dts
ATTACKER_OBJECTS    := $(BUILD_DIR)/attacker-startup.o \
	$(BUILD_DIR)/attacker-main.o $(BUILD_DIR)/attacker-trap.o

all: $(ATTACKER_BIN)

$(GENERATED_DTS): device-tree.dts.S | $(BUILD_DIR)
	$(CC) -E -P -x assembler-with-cpp \
		-DATTACKER_ADDRESS=$(ATTACKER_ADDRESS) \
		-DCOORDINATION_ADDRESS=$(COORDINATION_ADDRESS) \
		$< -o $@

$(DTB): $(GENERATED_DTS)
	dtc -i $(ROOT) -I dts -O dtb -o $@ $<

$(BUILD_DIR)/attacker-startup.o: $(GUEST_DIR)/startup.S | $(BUILD_DIR)
	$(CC) $(ASFLAGS) $(GUEST_ASFLAGS) -c $< -o $@

$(BUILD_DIR)/attacker-main.o: attacker.c coordination.h $(GUEST_LIB) | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) -I. -c $< -o $@

$(BUILD_DIR)/attacker-trap.o: attacker-trap.S | $(BUILD_DIR)
	$(CC) $(ASFLAGS) -c $< -o $@

$(ATTACKER_ELF): $(ATTACKER_OBJECTS) attacker.ld $(GUEST_LIB)
	$(CC) $(LDFLAGS) -Tattacker.ld $(ATTACKER_OBJECTS) $(GUEST_LDFLAGS) -o $@

$(ATTACKER_BIN): $(ATTACKER_ELF)
	$(OBJCOPY) -O binary $< $@

# The shared launcher builds $(BIN) before its inherited `run` target. Make
# the externally loaded attacker image part of that build graph as well.
$(BIN): $(ATTACKER_BIN)
