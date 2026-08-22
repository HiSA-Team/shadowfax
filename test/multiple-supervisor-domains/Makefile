ROOT                := ../..
LAUNCHER_NAME       := multiple-supervisor-domains
BUILD_DIR           ?= $(ROOT)/target/$(LAUNCHER_NAME)
LAUNCHER_METADATA   := 1M
GUEST_RAM_SIZE      ?= 4194304
GUEST_MEMORY_COUNT  := 2
PAYLOAD_SRC         := guest.c
EXTERNAL_TSM        ?= $(ROOT)/bin/tsm.elf
EXTERNAL_TSM_SIGNATURE ?= $(ROOT)/bin/tsm.bin.signature
EXTERNAL_TSM_ADDR   ?= 0x94000000
EXTERNAL_TSM_SIGNATURE_ADDR ?= 0x95fff000
RUN_QEMU_FLAGS      = $(QEMU_FLAGS) \
	-device loader,file=$(abspath $(EXTERNAL_TSM)),addr=$(EXTERNAL_TSM_ADDR),force-raw=on \
	-device loader,file=$(abspath $(EXTERNAL_TSM_SIGNATURE)),addr=$(EXTERNAL_TSM_SIGNATURE_ADDR),force-raw=on

ifeq ($(origin DTB), undefined)
DTB                 := $(BUILD_DIR)/device-tree.dtb
DTB_GENERATED       := 1
endif

include $(ROOT)/guests/bare-metal/launcher.mk

GENERATED_DTS := $(BUILD_DIR)/device-tree.dts

$(FDT_IMAGE):
	@test -f $@ || { \
		echo "missing staged platform artifact: $@" >&2; \
		echo "run: make -C $(ROOT) PYTHON='uv run --with cbor2' PLATFORM=$(PLATFORM) firmware" >&2; \
		exit 1; \
	}

ifeq ($(DTB_GENERATED),1)
$(GENERATED_DTS): device-tree.dts.S $(ROOT)/platform/generic/device-tree.dts \
		$(EXTERNAL_TSM) $(EXTERNAL_TSM_SIGNATURE) | $(BUILD_DIR)
	@image_size=$$(stat -c %s $(EXTERNAL_TSM)); \
	signature_size=$$(stat -c %s $(EXTERNAL_TSM_SIGNATURE)); \
	test $$image_size -le $$((0x1fff000)) || { echo "external TSM exceeds staging memory" >&2; exit 1; }; \
	$(CC) -E -P -x assembler-with-cpp \
		-DEXTERNAL_TSM_ADDR=$(EXTERNAL_TSM_ADDR) \
		-DEXTERNAL_TSM_SIZE=$$image_size \
		-DEXTERNAL_TSM_SIGNATURE_ADDR=$(EXTERNAL_TSM_SIGNATURE_ADDR) \
		-DEXTERNAL_TSM_SIGNATURE_SIZE=$$signature_size \
		$< -o $@

$(DTB): $(GENERATED_DTS)
	dtc -i $(ROOT) -I dts -O dtb -o $@ $<
endif
