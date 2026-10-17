ROOT          := ../../..
LAUNCHER_NAME := memory-ownership-consistency
BUILD_DIR     ?= $(ROOT)/target/security/memory-ownership-consistency

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
