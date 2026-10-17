ROOT          := ../../..
LAUNCHER_NAME := reclaimed-memory-zero
BUILD_DIR     ?= $(ROOT)/target/security/reclaimed-memory-zero
GUEST_ELF     := $(BUILD_DIR)/secret-guest.out
PAYLOAD_SRC   := guest.c

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
