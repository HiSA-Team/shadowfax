ROOT                := ../../..
LAUNCHER_NAME       := malicious-tvm
BUILD_DIR           ?= $(ROOT)/target/security/malicious-tvm
GUEST_MEMORY_SIZE   ?= 4194304
GUEST_MEMORY_COUNT := 2
PAYLOAD_SRC         := guest.c

include $(ROOT)/guests/bare-metal/launcher.mk
