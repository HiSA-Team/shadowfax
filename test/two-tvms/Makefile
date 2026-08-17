ROOT         := ../..
include      $(ROOT)/config.mk

BUILD_DIR    ?= $(ROOT)/target/two-tvms

FIRMWARE     ?= $(ROOT)/target/$(TARGET_TRIPLET)/$(PROFILE)/shadowfax
DICE_INPUT   ?= $(ROOT)/bin/shadowfax.dice.bin
DTB          ?= $(ROOT)/bin/device-tree.dtb

ELF          := $(BUILD_DIR)/two-tvms
BIN          := $(ELF).bin
GUEST_ELF    := $(BUILD_DIR)/guest.elf
OBJECTS      := $(BUILD_DIR)/startup.o $(BUILD_DIR)/main.o

EMBED_FLAGS  := -DGUEST_ELF=\"$(abspath $(GUEST_ELF))\"

.PHONY: all clean run

all: $(BIN)

$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/guest.o: guest.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/guest_startup.o: guest_startup.S | $(BUILD_DIR)
	$(CC) $(ASFLAGS) -c $< -o $@

$(GUEST_ELF): $(BUILD_DIR)/guest.o $(BUILD_DIR)/guest_startup.o guest_linker.ld
	$(CC) $(LDFLAGS) -Tguest_linker.ld \
		$(BUILD_DIR)/guest_startup.o $(BUILD_DIR)/guest.o -o $@

$(BUILD_DIR)/startup.o: startup.S $(GUEST_ELF) | $(BUILD_DIR)
	$(CC) $(ASFLAGS) $(EMBED_FLAGS) -c $< -o $@

$(BUILD_DIR)/main.o: main.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(ELF): $(OBJECTS) linker.ld $(DTB)
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
			echo "two-tvms requires contiguous untrusted memory regions" >&2; \
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
			-Tlinker.ld $(OBJECTS) -o $@

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
