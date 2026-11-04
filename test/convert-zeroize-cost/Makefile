ROOT                := ../..
LAUNCHER_NAME       := convert-zeroize-cost
BUILD_DIR           ?= $(ROOT)/target/$(LAUNCHER_NAME)
GUEST_RAM_SIZE      ?= 2097152
RUN_QEMU_FLAGS       = $(QEMU_FLAGS) -monitor none -no-reboot

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
