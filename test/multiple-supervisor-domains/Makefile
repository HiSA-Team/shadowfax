ROOT                := ../..
LAUNCHER_NAME       := multiple-supervisor-domains
BUILD_DIR           ?= $(ROOT)/target/$(LAUNCHER_NAME)
LAUNCHER_METADATA   := 1M
GUEST_RAM_SIZE      ?= 2097152
GUEST_MEMORY_COUNT  := 2
PAYLOAD_SRC         := guest.c

# The secondary trusted domain stages its own copy of the same TSM; layout.py
# fills in both sizes from the staged artifacts.
EXTERNAL_TSM        ?= $(TSM_IMAGE)
EXTERNAL_TSM_SIG    ?= $(TSM_SIG)

RUN_QEMU_FLAGS       = $(QEMU_FLAGS) \
	-device loader,file=$(abspath $(EXTERNAL_TSM)),addr=$(SHADOWFAX_DOMAIN_TRUSTED_DOMAIN_SECONDARY_TSM_IMAGE_ADDR),force-raw=on \
	-device loader,file=$(abspath $(EXTERNAL_TSM_SIG)),addr=$(SHADOWFAX_DOMAIN_TRUSTED_DOMAIN_SECONDARY_TSM_SIGNATURE_ADDR),force-raw=on

LAUNCHER_DOMAIN     := UNTRUSTED_DOMAIN
LAUNCHER_RAM_REGION := UMEM
GUEST_RAM_REGION    := UMEM_HIGH

include $(ROOT)/guests/bare-metal/launcher.mk
