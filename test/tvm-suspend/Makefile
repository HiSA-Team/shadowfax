ROOT          := ../..
LAUNCHER_NAME := tvm-suspend
PAYLOAD_SRC   := guest.c

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
