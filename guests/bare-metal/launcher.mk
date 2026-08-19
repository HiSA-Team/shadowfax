include $(ROOT)/config.mk

LAUNCHER_NAME       ?= launcher
BUILD_DIR           ?= $(ROOT)/target/$(LAUNCHER_NAME)
FIRMWARE            ?= $(ROOT)/target/$(TARGET_TRIPLET)/$(PROFILE)/shadowfax
DICE_INPUT          ?= $(ROOT)/bin/shadowfax.dice.bin
DTB                 ?= $(FDT_IMAGE)
LAUNCHER_MAIN       ?= main.c
LAUNCHER_METADATA   ?= 512K
GUEST_MEMORY_SIZE   ?= 4194304
GUEST_MEMORY_COUNT  ?= 1
GUEST_IN_RAM        ?= 1
LAUNCHER_EXTRA_CFLAGS ?=
LAUNCHER_EXTRA_CFLAGS += -DGUEST_RAM_SIZE=$(GUEST_MEMORY_SIZE)
LAUNCHER_EXTRA_LDFLAGS ?=
RUN_QEMU_FLAGS      ?= $(QEMU_FLAGS)
LOAD_ADDR           ?=

ELF                 ?= $(BUILD_DIR)/$(LAUNCHER_NAME)
BIN                 ?= $(ELF).bin
OBJECTS             := $(BUILD_DIR)/startup.o $(BUILD_DIR)/main.o
GUEST_CONFIG_STAMP  := $(BUILD_DIR)/.guest-memory
GUEST_LIB_INPUTS    := $(GUEST_DIR)/lib/baremetal.c \
	$(GUEST_DIR)/lib/baremetal_elf.c \
	$(GUEST_DIR)/include/baremetal.h \
	$(GUEST_DIR)/include/baremetal_elf.h

ifneq ($(strip $(PAYLOAD_SRC)),)
GUEST_ELF           ?= $(BUILD_DIR)/guest.elf
PAYLOAD_OBJECT      := $(BUILD_DIR)/guest.o
endif

EMBED_FLAGS         :=
EMBED_INPUTS        :=
ifneq ($(strip $(GUEST_ELF)),)
EMBED_FLAGS         += -DGUEST_ELF=\"$(abspath $(GUEST_ELF))\"
EMBED_INPUTS        += $(GUEST_ELF)
endif
ifneq ($(strip $(GUEST_DTB)),)
EMBED_FLAGS         += -DGUEST_DTB=\"$(abspath $(GUEST_DTB))\"
EMBED_INPUTS        += $(GUEST_DTB)
endif

.PHONY: all clean run FORCE

all: $(BIN)

$(BUILD_DIR):
	mkdir -p $@

$(FIRMWARE) $(DICE_INPUT) $(DTB):
	@test -f $@ || { \
		echo "missing staged platform artifact: $@" >&2; \
		echo "run: make -C $(ROOT) PYTHON='uv run --with cbor2' PLATFORM=$(PLATFORM) firmware" >&2; \
		exit 1; \
	}

FORCE:

$(GUEST_CONFIG_STAMP): FORCE | $(BUILD_DIR)
	@expected='$(GUEST_MEMORY_SIZE) $(GUEST_MEMORY_COUNT)'; \
	actual=''; test ! -f $@ || read -r actual < $@; \
	test "$$actual" = "$$expected" || printf '%s\n' "$$expected" > $@

$(GUEST_LIB): $(GUEST_LIB_INPUTS)
	$(MAKE) -C $(GUEST_DIR) $(GUEST_LIB)

$(BUILD_DIR)/startup.o: $(GUEST_DIR)/startup.S $(EMBED_INPUTS) | $(BUILD_DIR)
	$(CC) $(ASFLAGS) $(GUEST_ASFLAGS) $(EMBED_FLAGS) -c $< -o $@

$(BUILD_DIR)/main.o: $(LAUNCHER_MAIN) $(GUEST_LIB) $(GUEST_CONFIG_STAMP) | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) $(LAUNCHER_EXTRA_CFLAGS) -c $< -o $@

ifneq ($(strip $(PAYLOAD_SRC)),)
$(PAYLOAD_OBJECT): $(PAYLOAD_SRC) $(GUEST_LIB) | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) $(PAYLOAD_CFLAGS) -c $< -o $@

$(BUILD_DIR)/guest_startup.o: $(GUEST_DIR)/vmstartup.S | $(BUILD_DIR)
	$(CC) $(ASFLAGS) $(GUEST_ASFLAGS) -c $< -o $@

$(GUEST_ELF): $(PAYLOAD_OBJECT) $(BUILD_DIR)/guest_startup.o \
		$(GUEST_DIR)/linker.ld $(GUEST_LIB)
	$(CC) $(LDFLAGS) -T$(GUEST_DIR)/linker.ld \
		$(BUILD_DIR)/guest_startup.o $(PAYLOAD_OBJECT) $(GUEST_LDFLAGS) -o $@
endif

$(ELF): $(OBJECTS) $(GUEST_DIR)/launcher.ld $(DTB) $(GUEST_LIB)
	@set -- $$(fdtget -t x $(DTB) /chosen/shadowfax host-fdt); \
	host_fdt_addr=$$((0x$$1 << 32 | 0x$$2)); \
	set -- $$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem base); \
	ram_base=$$((0x$$1 << 32 | 0x$$2)); \
	ram_low_order=$$((0x$$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem order))); \
	set -- $$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem-high base); \
	guest_base=$$((0x$$1 << 32 | 0x$$2)); \
	guest_order=$$((0x$$(fdtget -t x $(DTB) /chosen/opensbi-domains/umem-high order))); \
	guest_region_size=$$((1 << guest_order)); \
	if test "$(GUEST_IN_RAM)" = 1; then \
		ram_low_end=$$((ram_base + (1 << ram_low_order))); \
		test $$ram_low_end -eq $$guest_base || { \
			echo "$(LAUNCHER_NAME) requires contiguous untrusted memory regions" >&2; \
			exit 1; \
		}; \
		ram_size=$$((guest_base + guest_region_size - ram_base)); \
	else \
		ram_size=$$((1 << ram_low_order)); \
	fi; \
	$(CC) $(LDFLAGS) $(LAUNCHER_EXTRA_LDFLAGS) \
		-Wl,--defsym=SHADOWFAX_RAM_BASE=$$ram_base \
		-Wl,--defsym=SHADOWFAX_RAM_SIZE=$$ram_size \
		-Wl,--defsym=SHADOWFAX_HOST_FDT_ADDR=$$host_fdt_addr \
		-Wl,--defsym=SHADOWFAX_GUEST_BASE=$$guest_base \
		-Wl,--defsym=SHADOWFAX_GUEST_UNIT_SIZE=$(GUEST_MEMORY_SIZE) \
		-Wl,--defsym=SHADOWFAX_GUEST_SIZE=$(GUEST_MEMORY_SIZE)*$(GUEST_MEMORY_COUNT) \
		-Wl,--defsym=SHADOWFAX_GUEST_REGION_SIZE=$$guest_region_size \
		-Wl,--defsym=SHADOWFAX_METADATA_SIZE=$(LAUNCHER_METADATA) \
		-Wl,--defsym=SHADOWFAX_GUEST_IN_RAM=$(GUEST_IN_RAM) \
		-T$(GUEST_DIR)/launcher.ld $(OBJECTS) $(GUEST_LDFLAGS) -o $@

$(BIN): $(ELF)
	$(OBJCOPY) -O binary $< $@

run: $(BIN) $(FIRMWARE) $(DICE_INPUT) $(DTB)
	@set -- $$(fdtget -t x $(DTB) /chosen/shadowfax dice-input); \
	dice_input_addr=$$((0x$$1 << 32 | 0x$$2)); \
	set -- $$(fdtget -t x $(DTB) /chosen/opensbi-domains/untrusted-domain next-addr); \
	boot_domain_address=$$((0x$$1 << 32 | 0x$$2)); \
	load_addr='$(LOAD_ADDR)'; test -n "$$load_addr" || load_addr="$$boot_domain_address"; \
	$(QEMU) $(RUN_QEMU_FLAGS) \
		-bios $(FIRMWARE) \
		-dtb $(DTB) \
		-device loader,file=$(DICE_INPUT),addr=$$dice_input_addr,force-raw=on \
		-device loader,file=$(BIN),addr=$$load_addr,force-raw=on

clean:
	$(RM) -r $(BUILD_DIR)
