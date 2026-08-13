ROOT         := ../../..
include $(ROOT)/config.mk

LAUNCHER_DIR := $(ROOT)/test/standalone-tvm-launcher
BUILD_DIR    ?= $(ROOT)/target/security/reclaimed-memory-zero

FIRMWARE     ?= $(ROOT)/target/$(TARGET_TRIPLET)/$(PROFILE)/shadowfax
DICE_INPUT   ?= $(ROOT)/bin/shadowfax.dice.bin
DTB          ?= $(ROOT)/bin/device-tree.dtb

ELF          := $(BUILD_DIR)/reclaimed-memory-zero
BIN          := $(ELF).bin
GUEST_ELF    := $(BUILD_DIR)/secret-guest.out
OBJECTS      := $(BUILD_DIR)/startup.o $(BUILD_DIR)/main.o \
	$(BUILD_DIR)/launcher.o

EMBED_FLAGS  := -DGUEST_ELF=\"$(abspath $(GUEST_ELF))\"

.PHONY: all clean run

all: $(BIN)

$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/startup.o: $(LAUNCHER_DIR)/startup.S $(GUEST_ELF) | $(BUILD_DIR)
	$(CC) $(ASFLAGS) $(EMBED_FLAGS) -c $< -o $@

$(BUILD_DIR)/main.o: main.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/launcher.o: $(LAUNCHER_DIR)/main.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -Dmain=security_launcher_main -c $< -o $@

$(GUEST_ELF): guest.c $(ROOT)/guests/bare-metal/vmstartup.S \
		$(ROOT)/guests/bare-metal/linker.ld | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(LDFLAGS) -T$(ROOT)/guests/bare-metal/linker.ld \
		guest.c $(ROOT)/guests/bare-metal/vmstartup.S -o $@

$(ELF): $(OBJECTS) $(LAUNCHER_DIR)/linker.ld $(DTB)
	@set -- $$(fdtget -t x $(DTB) /chosen/shadowfax host-fdt); \
	host_fdt_addr=$$((0x$$1 << 32 | 0x$$2)); \
	set -- $$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem base); \
	ram_base=$$((0x$$1 << 32 | 0x$$2)); \
	ram_low_order=$$((0x$$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem order))); \
	set -- $$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem-high base); \
	guest_base=$$((0x$$1 << 32 | 0x$$2)); \
	guest_order=$$((0x$$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem-high order))); \
	ram_low_end=$$((ram_base + (1 << ram_low_order))); \
	test $$ram_low_end -eq $$guest_base || { \
		echo "security launcher requires contiguous umem and umem-high regions" >&2; \
		exit 1; \
	}; \
	guest_size=$$((1 << guest_order)); \
	ram_size=$$((guest_base + guest_size - ram_base)); \
	$(CC) $(LDFLAGS) \
		-Wl,--defsym=SHADOWFAX_RAM_BASE=$$ram_base \
		-Wl,--defsym=SHADOWFAX_RAM_SIZE=$$ram_size \
		-Wl,--defsym=SHADOWFAX_HOST_FDT_ADDR=$$host_fdt_addr \
		-Wl,--defsym=SHADOWFAX_GUEST_BASE=$$guest_base \
		-Wl,--defsym=SHADOWFAX_GUEST_SIZE=$$guest_size \
		-T$(LAUNCHER_DIR)/linker.ld $(OBJECTS) -o $@

$(BIN): $(ELF)
	$(OBJCOPY) -O binary $< $@

run: $(BIN) $(FIRMWARE) $(DICE_INPUT) $(DTB)
	@set -- $$(fdtget -t x $(DTB) /chosen/shadowfax dice-input); \
	dice_input_addr=$$((0x$$1 << 32 | 0x$$2)); \
	set -- $$(fdtget -t x $(DTB) /chosen/opensbi-domains/untrusted-domain next-addr); \
	boot_domain_address=$$((0x$$1 << 32 | 0x$$2)); \
	$(QEMU) $(QEMU_FLAGS) \
		-bios $(FIRMWARE) \
		-dtb $(DTB) \
		-device loader,file=$(DICE_INPUT),addr=$$dice_input_addr,force-raw=on \
		-device loader,file=$(BIN),addr=$$boot_domain_address,force-raw=on

clean:
	$(RM) -r $(BUILD_DIR)
