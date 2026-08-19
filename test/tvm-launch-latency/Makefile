ROOT                := ../..
LAUNCHER_NAME       := tvm-launch-latency
BUILD_DIR           ?= $(ROOT)/target/$(LAUNCHER_NAME)
LAUNCHER_METADATA   := 1M
GUEST_RAM_SIZE      := 268435456
GUEST_IN_RAM        := 0
RUN_QEMU_FLAGS      := -M virt -m 1G -smp 1 -nographic -monitor none -no-reboot

ifeq ($(origin DTB), undefined)
DTB                 := $(BUILD_DIR)/device-tree.dtb
DTB_GENERATED       := 1
endif

include $(ROOT)/guests/bare-metal/launcher.mk

$(FDT_IMAGE):
	@test -f $@ || { \
		echo "missing staged platform artifact: $@" >&2; \
		echo "run: make -C $(ROOT) PYTHON='uv run --with cbor2' PLATFORM=$(PLATFORM) firmware" >&2; \
		exit 1; \
	}

ifeq ($(DTB_GENERATED),1)
$(DTB): $(FDT_IMAGE) | $(BUILD_DIR)
	cp $< $@
	fdtput -t x $@ /chosen/opensbi-domains/umem-high base 0 0xa0000000
	fdtput -t x $@ /chosen/opensbi-domains/umem-high order 0x1c
endif
