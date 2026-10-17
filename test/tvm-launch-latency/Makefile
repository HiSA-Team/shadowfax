ROOT                := ../..
LAUNCHER_NAME       := tvm-launch-latency
BUILD_DIR           ?= $(ROOT)/target/$(LAUNCHER_NAME)
LAUNCHER_METADATA   := 1M
GUEST_RAM_SIZE      ?= 33554432
RUN_QEMU_FLAGS       = $(QEMU_FLAGS) -monitor none -no-reboot

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
