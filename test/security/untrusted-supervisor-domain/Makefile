ROOT                := ../../..
LAUNCHER_NAME       := untrusted-supervisor-domain
BUILD_DIR           ?= $(ROOT)/target/security/$(LAUNCHER_NAME)
# QEMU's normal RISC-V boot ROM starts every hart at the beginning of a raw
# firmware image. Shadowfax therefore runs here as a conventional BIOS image,
# rather than through per-hart loader devices.
FIRMWARE              = $(BIN_DIR)/shadowfax.bin
GUEST_RAM_SIZE      := 2097152
GUEST_MEMORY_COUNT  := 1
PAYLOAD_SRC         := guest.c
ATTACKER_ADDRESS     = $(SHADOWFAX_REGION_ATTACKER_MEMORY_BASE)
ATTACKER_ELF        := $(BUILD_DIR)/attacker
ATTACKER_BIN        := $(ATTACKER_ELF).bin
RUN_QEMU_FLAGS       = -M virt -m $(SHADOWFAX_QEMU_MEM) -smp $(SHADOWFAX_SMP) -nographic -monitor none -no-reboot \
	-device loader,file=$(abspath $(ATTACKER_BIN)),addr=$(ATTACKER_ADDRESS),force-raw=on

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk

ATTACKER_OBJECTS    := $(BUILD_DIR)/attacker-startup.o \
	$(BUILD_DIR)/attacker-main.o $(BUILD_DIR)/attacker-trap.o

all: $(ATTACKER_BIN)



$(BUILD_DIR)/attacker-startup.o: $(GUEST_DIR)/startup.S | $(BUILD_DIR)
	$(CC) $(ASFLAGS) $(GUEST_ASFLAGS) -c $< -o $@

$(BUILD_DIR)/attacker-main.o: attacker.c coordination.h $(LAYOUT_MK) $(GUEST_LIB) | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) -I. -I$(LAYOUT_H_DIR) $(DEPFLAGS) -c $< -o $@

$(BUILD_DIR)/attacker-trap.o: attacker-trap.S | $(BUILD_DIR)
	$(CC) $(ASFLAGS) -c $< -o $@

$(ATTACKER_ELF): $(ATTACKER_OBJECTS) attacker.ld $(GUEST_LIB)
	$(CC) $(LDFLAGS) \
		-Wl,--defsym=SHADOWFAX_ATTACKER_BASE=$(SHADOWFAX_REGION_ATTACKER_MEMORY_BASE) \
		-Wl,--defsym=SHADOWFAX_ATTACKER_SIZE=$(SHADOWFAX_REGION_ATTACKER_MEMORY_SIZE) \
		-Tattacker.ld \
		$(ATTACKER_OBJECTS) $(GUEST_LDFLAGS) -o $@

$(ATTACKER_BIN): $(ATTACKER_ELF)
	$(OBJCOPY) -O binary $< $@

# The shared launcher builds $(BIN) before its inherited `run` target. Make
# the externally loaded attacker image part of that build graph as well.
$(BIN): $(ATTACKER_BIN)
