LAUNCHER_NAME       ?= launcher
BUILD_DIR           ?= $(ROOT)/target/$(LAUNCHER_NAME)
LAYOUT_DIR          ?= $(BUILD_DIR)

include $(ROOT)/config.mk

FIRMWARE            ?= $(ROOT)/target/$(TARGET_TRIPLET)/$(PROFILE)/shadowfax
DICE_INPUT          ?= $(BIN_DIR)/shadowfax.dice.bin
TSM_IMAGE           ?= $(BIN_DIR)/tsm.elf
TSM_SIG             ?= $(BIN_DIR)/tsm.bin.signature
DTB                 ?= $(LAYOUT_DIR)/device-tree.dtb
LAUNCHER_DTS        ?= device-tree.dts
LAUNCHER_MAIN       ?= main.c
LAUNCHER_METADATA   ?= 512K
GUEST_RAM_SIZE      ?= 2097152
GUEST_MEMORY_COUNT  ?= 1
LAUNCHER_EXTRA_CFLAGS ?=
LAUNCHER_EXTRA_CFLAGS += -DGUEST_RAM_SIZE=$(GUEST_RAM_SIZE)
LAUNCHER_EXTRA_LDFLAGS ?=
RUN_QEMU_FLAGS      ?= $(QEMU_FLAGS)
LOAD_ADDR           ?=

# Selectors are DTS node names in export form: uppercase, with '-' changed to '_'.
LAUNCHER_RAM_BASE    = $(SHADOWFAX_REGION_$(LAUNCHER_RAM_REGION)_BASE)
LAUNCHER_RAM_SIZE    = $(SHADOWFAX_REGION_$(LAUNCHER_RAM_REGION)_SIZE)
LAUNCHER_LOAD_ADDR   = $(if $(LOAD_ADDR),$(LOAD_ADDR),$(SHADOWFAX_DOMAIN_$(LAUNCHER_DOMAIN)_ENTRY))
GUEST_REGION_BASE    = $(SHADOWFAX_REGION_$(GUEST_RAM_REGION)_BASE)
GUEST_REGION_SIZE    = $(SHADOWFAX_REGION_$(GUEST_RAM_REGION)_SIZE)

SHADOWFAX_LOADERS    = \
	-device loader,file=$(DICE_INPUT),addr=$(SHADOWFAX_DICE_ADDR),force-raw=on \
	-device loader,file=$(TSM_IMAGE),addr=$(SHADOWFAX_TSM_IMAGE_ADDR),force-raw=on \
	-device loader,file=$(TSM_SIG),addr=$(SHADOWFAX_TSM_SIGNATURE_ADDR),force-raw=on

ELF                 ?= $(BUILD_DIR)/$(LAUNCHER_NAME)
BIN                 ?= $(ELF).bin
OBJECTS             := $(BUILD_DIR)/startup.o $(BUILD_DIR)/main.o
DEPFLAGS            := -MMD -MP
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

# Every example owns its device tree and the matching layout exports.
$(LAYOUT_MK): $(LAUNCHER_DTS) $(TSM_IMAGE) $(TSM_SIG) $(LAYOUT_PY)
	@mkdir -p $(@D)
	$(PYTHON3) $(LAYOUT_PY) $(LAUNCHER_DTS) $(LAYOUT_DIR) --include $(ROOT) --artifacts $(BIN_DIR)
$(DTB): $(LAYOUT_MK)

# make remakes every included file before running any goal, so an unguarded
# include would compile the tree just to delete it again -- and fail outright
# when the staged artifacts the layout rule needs are absent. `clean` needs
# nothing from the exports, so only include them when something else is asked.
LAYOUT_NEEDED := $(filter-out clean,$(if $(MAKECMDGOALS),$(MAKECMDGOALS),all))
ifneq (,$(LAYOUT_NEEDED))
-include $(LAYOUT_DIR)/layout.d
include $(LAYOUT_MK)

# The DTS node names are this fragment's API: LAUNCHER_RAM_REGION := UMEM
# resolves to $(SHADOWFAX_REGION_UMEM_BASE). A misspelled selector used to reach
# the linker as an empty --defsym, so check each one against the names the tree
# declares and say what those are.
#
# SHADOWFAX_REGIONS is empty on the pass that generates layout.mk: make reads
# the makefiles, remakes the include, then restarts. Only check once the
# exports are actually in hand, which is the restart pass.
check-selector = $(if $(filter $(2),$(3)),,$(error $(LAUNCHER_NAME): \
$(1)='$(2)' is not $(4) declared by $(LAUNCHER_DTS); available: $(3)))
ifneq (,$(SHADOWFAX_RAM_REGIONS))
$(call check-selector,LAUNCHER_RAM_REGION,$(LAUNCHER_RAM_REGION),$(SHADOWFAX_RAM_REGIONS),a RAM region)
$(call check-selector,GUEST_RAM_REGION,$(GUEST_RAM_REGION),$(SHADOWFAX_RAM_REGIONS),a RAM region)
ifeq (,$(strip $(LOAD_ADDR)))
$(call check-selector,LAUNCHER_DOMAIN,$(LAUNCHER_DOMAIN),$(SHADOWFAX_DOMAINS),a domain)
endif
endif
endif

$(FIRMWARE) $(DICE_INPUT) $(TSM_IMAGE) $(TSM_SIG):
	@test -f $@ || { \
		echo "missing staged platform artifact: $@" >&2; \
		echo "run: make -C $(ROOT) PLATFORM=$(PLATFORM) firmware" >&2; \
		exit 1; \
	}

FORCE:

$(GUEST_CONFIG_STAMP): FORCE
	@mkdir -p $(@D)
	@expected='$(GUEST_RAM_SIZE) $(GUEST_MEMORY_COUNT)'; \
	actual=''; test ! -f $@ || read -r actual < $@; \
	test "$$actual" = "$$expected" || printf '%s\n' "$$expected" > $@

$(GUEST_LIB): $(GUEST_LIB_INPUTS)
	$(MAKE) -C $(GUEST_DIR) $(GUEST_LIB)

$(BUILD_DIR)/startup.o: $(GUEST_DIR)/startup.S $(EMBED_INPUTS)
	@mkdir -p $(@D)
	$(CC) $(ASFLAGS) $(GUEST_ASFLAGS) $(EMBED_FLAGS) -c $< -o $@

$(BUILD_DIR)/main.o: $(LAUNCHER_MAIN) $(LAYOUT_MK) $(GUEST_LIB) $(GUEST_CONFIG_STAMP)
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) $(LAUNCHER_EXTRA_CFLAGS) $(DEPFLAGS) -c $< -o $@

ifneq ($(strip $(PAYLOAD_SRC)),)
$(PAYLOAD_OBJECT): $(PAYLOAD_SRC) $(GUEST_LIB)
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) $(PAYLOAD_CFLAGS) $(DEPFLAGS) -c $< -o $@

$(BUILD_DIR)/guest_startup.o: $(GUEST_DIR)/vmstartup.S
	@mkdir -p $(@D)
	$(CC) $(ASFLAGS) $(GUEST_ASFLAGS) -c $< -o $@

$(GUEST_ELF): $(PAYLOAD_OBJECT) $(BUILD_DIR)/guest_startup.o \
		$(GUEST_DIR)/linker.ld $(GUEST_LIB)
	@mkdir -p $(@D)
	$(CC) $(LDFLAGS) -T$(GUEST_DIR)/linker.ld \
		$(BUILD_DIR)/guest_startup.o $(PAYLOAD_OBJECT) $(GUEST_LDFLAGS) -o $@
endif

$(ELF): $(OBJECTS) $(GUEST_DIR)/launcher.ld $(LAYOUT_MK) $(GUEST_LIB)
	@mkdir -p $(@D)
	@test -n '$(LAUNCHER_LOAD_ADDR)' -a -n '$(LAUNCHER_RAM_BASE)' -a -n '$(GUEST_REGION_BASE)' || { \
		echo "$(LAUNCHER_NAME): select a domain, launcher RAM region and guest RAM region from the DTS" >&2; \
		exit 1; \
	}
	$(CC) $(LDFLAGS) $(LAUNCHER_EXTRA_LDFLAGS) \
		-Wl,--defsym=SHADOWFAX_RAM_BASE=$(LAUNCHER_RAM_BASE) \
		-Wl,--defsym=SHADOWFAX_RAM_SIZE=$(LAUNCHER_RAM_SIZE) \
		-Wl,--defsym=SHADOWFAX_FDT_ADDR=$(SHADOWFAX_FDT_ADDR) \
		-Wl,--defsym=SHADOWFAX_GUEST_BASE=$(GUEST_REGION_BASE) \
		-Wl,--defsym=SHADOWFAX_GUEST_UNIT_SIZE=$(GUEST_RAM_SIZE) \
		-Wl,--defsym=SHADOWFAX_GUEST_SIZE=$(GUEST_RAM_SIZE)*$(GUEST_MEMORY_COUNT) \
		-Wl,--defsym=SHADOWFAX_GUEST_REGION_SIZE=$(GUEST_REGION_SIZE) \
		-Wl,--defsym=SHADOWFAX_METADATA_SIZE=$(LAUNCHER_METADATA) \
		-T$(GUEST_DIR)/launcher.ld $(OBJECTS) $(GUEST_LDFLAGS) -o $@

$(BIN): $(ELF)
	@mkdir -p $(@D)
	$(OBJCOPY) -O binary $< $@

run: $(BIN) $(FIRMWARE) $(DICE_INPUT) $(TSM_IMAGE) $(TSM_SIG) $(DTB) $(LAYOUT_MK)
	$(QEMU) $(RUN_QEMU_FLAGS) \
		-bios $(FIRMWARE) \
		-dtb $(DTB) \
		$(SHADOWFAX_LOADERS) \
		-device loader,file=$(BIN),addr=$(LAUNCHER_LOAD_ADDR),force-raw=on

clean:
	$(RM) -r $(BUILD_DIR)

-include $(OBJECTS:.o=.d) $(PAYLOAD_OBJECT:.o=.d)
