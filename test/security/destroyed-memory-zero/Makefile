ROOT          := ../../..
LAUNCHER_NAME := destroyed-memory-zero
BUILD_DIR     ?= $(ROOT)/target/security/destroyed-memory-zero
GUEST_ELF     := $(BUILD_DIR)/scanner-guest.out
PAYLOAD_SRC   := guest.c

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
