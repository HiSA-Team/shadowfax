ROOT          := ../../..
LAUNCHER_NAME := converted-memory-isolation
BUILD_DIR     ?= $(ROOT)/target/security/converted-memory-isolation
PAYLOAD_SRC   := guest.c

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
