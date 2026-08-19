ROOT         := ../..
include $(ROOT)/config.mk

BUILD_DIR    ?= $(ROOT)/target/tvm-launch-latency
FIRMWARE     ?= $(ROOT)/target/$(TARGET_TRIPLET)/$(PROFILE)/shadowfax
DICE_INPUT   ?= $(ROOT)/bin/shadowfax.dice.bin
DTB          ?= $(ROOT)/bin/device-tree.dtb

ELF          = $(BUILD_DIR)/tvm-launch-latency
BIN          = $(ELF).bin
OBJECTS      = $(BUILD_DIR)/startup.o $(BUILD_DIR)/main.o

.PHONY: all clean run

all: $(BIN)

$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/startup.o: startup.S | $(BUILD_DIR)
	$(CC) $(ASFLAGS) -c $< -o $@

$(BUILD_DIR)/main.o: main.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(ELF): $(OBJECTS) linker.ld $(DTB)
	@set -- $$(fdtget -t x $(DTB) /chosen/shadowfax host-fdt); \
		host_fdt_addr=$$((0x$$1 << 32 | 0x$$2)); \
		set -- $$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem base); \
		ram_base=$$((0x$$1 << 32 | 0x$$2)); \
		ram_size=$$((1 << 0x$$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem order))); \
		set -- $$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem-high base); \
		guest_base=$$((0x$$1 << 32 | 0x$$2)); \
		guest_size=$$((1 << 0x$$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem-high order))); \
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
		load_addr=$$((0x$$1 << 32 | 0x$$2)); \
		$(QEMU) -M virt -m 1G -smp 1 -nographic -monitor none -no-reboot \
			-bios $(FIRMWARE) -dtb $(DTB) \
			-device loader,file=$(DICE_INPUT),addr=$$dice_input_addr,force-raw=on \
			-device loader,file=$(BIN),addr=$$load_addr,force-raw=on

clean:
	$(RM) -r $(BUILD_DIR)
