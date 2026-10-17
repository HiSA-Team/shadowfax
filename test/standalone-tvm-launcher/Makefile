ROOT          := ../..
LAUNCHER_NAME := standalone-tvm-launcher
GUEST_ELF     ?= $(ROOT)/guests/bare-metal/attestation.out
GUEST_DTB     ?=

LAUNCHER_DOMAIN     := RUNNER
LAUNCHER_RAM_REGION := RUNNER_RAM
GUEST_RAM_REGION    := GUEST_POOL

include $(ROOT)/guests/bare-metal/launcher.mk
