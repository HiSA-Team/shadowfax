ROOT         := ../..
include $(ROOT)/config.mk

BUILD_DIR    ?= $(ROOT)/target/standalone-tvm-launcher

# Inputs are built outside this Makefile and may be overridden from the CLI.
FIRMWARE     ?= $(ROOT)/target/$(TARGET_TRIPLET)/$(PROFILE)/shadowfax
DICE_INPUT   ?= $(ROOT)/bin/shadowfax.dice.bin
DTB          ?= $(ROOT)/bin/device-tree.dtb
GUEST_ELF    ?= $(ROOT)/guests/bare-metal/attestation.out
GUEST_DTB    ?=

LOAD_ADDR    ?= $(BOOT_DOMAIN_ADDRESS)

ELF           = $(BUILD_DIR)/standalone-tvm-launcher
BIN           = $(ELF).bin
OBJECTS       = $(BUILD_DIR)/startup.o $(BUILD_DIR)/main.o

EMBED_FLAGS   = -DGUEST_ELF=\"$(abspath $(GUEST_ELF))\"
EMBED_INPUTS  = $(GUEST_ELF)
ifneq ($(strip $(GUEST_DTB)),)
EMBED_FLAGS  += -DGUEST_DTB=\"$(abspath $(GUEST_DTB))\"
EMBED_INPUTS += $(GUEST_DTB)
endif

.PHONY: all clean run

all: $(BIN)

$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/startup.o: startup.S $(EMBED_INPUTS) | $(BUILD_DIR)
	$(CC) $(ASFLAGS) $(EMBED_FLAGS) -c $< -o $@

$(BUILD_DIR)/main.o: main.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(ELF): $(OBJECTS) linker.ld
	$(CC) $(LDFLAGS) -Tlinker.ld $(OBJECTS) -o $@

$(BIN): $(ELF)
	$(OBJCOPY) -O binary $< $@

# Run only with existing firmware inputs; this target never builds them.
run: $(BIN) $(FIRMWARE) $(DICE_INPUT) $(DTB)
	$(QEMU) $(QEMU_FLAGS) \
		-bios $(FIRMWARE) \
		-device loader,file=$(DTB),addr=$(FDT_ADDR),force-raw=on \
		-device loader,file=$(DICE_INPUT),addr=$(DICE_INPUT_ADDR),force-raw=on \
		-device loader,file=$(BIN),addr=$(LOAD_ADDR),force-raw=on

clean:
	$(RM) -r $(BUILD_DIR)
