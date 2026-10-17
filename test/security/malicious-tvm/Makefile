ROOT                := ../../..
LAUNCHER_NAME       := malicious-tvm
BUILD_DIR           ?= $(ROOT)/target/security/malicious-tvm
GUEST_RAM_SIZE      ?= 4194304
GUEST_MEMORY_COUNT := 2
PAYLOAD_SRC         := guest.c

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
