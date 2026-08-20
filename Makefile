# This Makefile contains everything needed to build and run Shadowfax with examples. This Makefile
# is the unique entrypoint for managing Shadowfax build since it detects and the host system and
# sets up reasonable defaults. Variable users may want to override:
#
# - RV_PREFIX:           specify with the path to the target riscv toolchain prefix
# - PLATFORM:            target platform, this is used for OpenSBI initialization
# - GDB_COVE_SCRIPT:     path to the example to run
# - QEMU_DEVICES:        additional QEMU device/loader arguments
#
# Usage:
#   make help # discover available targets
#   make qemu-run # runs the system on qemu (DEBUG=1 to start gdb server and wait)
#
# Author: Giuseppe Capasso <capassog97@gmail.com>

PYTHON            ?= python
RUSTFLAGS         ?= -C target-feature=+h
HOST_ARCHITECTURE := $(shell uname -m)
HOST_TRIPLET      := $(shell rustc -vV | awk '/^host:/ { print $$2 }')
HOST_LIBC         := $(shell if ldd --version 2>&1 | grep -q musl; then echo musl; else echo gnu; fi)
RV_PREFIX         ?= riscv64-unknown-linux-$(HOST_LIBC)-

include config.mk

OPENSBI_VERSION            := $(shell git -C shadowfax/opensbi describe)
OPENSBI_PATCH              := shadowfax/opensbi-sbi-domain-change-active.diff
QEMU_DEVICES               ?=

# Files and Directories
BIN_DIR                     = bin
TARGET_DIR                  = target/$(TARGET_TRIPLET)/$(PROFILE)
KEYS_DIR                    = shadowfax/keys
TEST_DIR                    = test/functional/
CARGO_FLAGS                 =

FW_ELF                      = $(TARGET_DIR)/shadowfax
FW_BIN                      = $(BIN_DIR)/shadowfax.bin
TSM_ELF                     = $(TARGET_DIR)/tsm
TSM_SIG                     = $(BIN_DIR)/tsm.bin.signature

# Keys and Dice files
DICE_INPUT                  = $(BIN_DIR)/shadowfax.dice.bin
FDT_SOURCE                  = platform/$(PLATFORM)/device-tree.dts
PRIVATE_KEY                 = $(KEYS_DIR)/privatekey.pem
PUBLIC_KEY                  = $(KEYS_DIR)/publickey.pem
DICE_PLATFORM_PUBLIC_KEY    = $(KEYS_DIR)/root_of_trust_pub.bin
DICE_PLATFORM_PRIVATE_KEY   = $(KEYS_DIR)/root_of_trust_priv.bin

# Debug variables for QEMU
GDB_SETTINGS_SCRIPT         := test/debug/gdbinit
GDB_COVE_SCRIPT             ?= test/debug/gdb_covh_get_tsm_info.py

# Needed for OpenSBI
export RV_PREFIX

# Needed to avoid passing manually to Cargo
export RUSTFLAGS

export PLATFORM
export FDT_IMAGE

ifeq ($(HOST_LIBC), musl)

$(warning Musl system detected. Make sure you provide the libclang.a path in 'scripts/llvm-config.sh' accordingly and provide the path do the build directory in LIBCLANG_STATIC_PATH variable)

ifeq ($(LIBCLANG_STATIC_PATH),)
$(error Missing LIBCLANG_STATIC_PATH environment variable which is required in a musl environment)
endif

export LLVM_CONFIG_PATH     := $(CURDIR)/scripts/llvm-config.sh
endif

.PHONY: all clean firmware tsm test generate-keys guests help opensbi-patch

## all: build tsm, firmware, attestation payload, and platform DTB
all: guests firmware build-info

## guests: build bare-metal guests in guests/bare-metal/
guests:
	$(MAKE) -C guests/

## firmware: builds the firmware, TSM, DICE input, and platform DTB
firmware: opensbi-patch $(DICE_INPUT) $(FDT_IMAGE)

opensbi-patch: $(OPENSBI_PATCH)
	@patch=$$(realpath $<); \
	git -C shadowfax/opensbi rev-parse --is-inside-work-tree >/dev/null 2>&1 || { \
		echo "OpenSBI submodule is not initialized; run: git submodule update --init shadowfax/opensbi" >&2; \
		exit 1; \
	}; \
	if git -C shadowfax/opensbi apply --reverse --check "$$patch"; then \
		:; \
	elif git -C shadowfax/opensbi apply --check "$$patch"; then \
		git -C shadowfax/opensbi apply "$$patch"; \
	else \
		echo "OpenSBI patch cannot be applied cleanly: $(OPENSBI_PATCH)" >&2; \
		exit 1; \
	fi

## tsm: build the TSM and signs it
tsm: $(TSM_SIG)

# create attestation input (CDI_ID and Certificate) according to DICE specification
$(DICE_INPUT): $(FW_BIN) | $(BIN_DIR)
	$(PYTHON) scripts/dice_tool.py generate-platform-token \
		--uds-private-key $(DICE_PLATFORM_PRIVATE_KEY) \
		--uds-public-key $(DICE_PLATFORM_PUBLIC_KEY) \
		$< $@

$(FDT_IMAGE): $(FDT_SOURCE)
	mkdir -p $(dir $@)
	dtc -I dts -O dtb -o $@ $<

$(FW_BIN): $(FW_ELF) | $(BIN_DIR)
	$(OBJCOPY) -O binary $< $@

$(FW_ELF): $(TSM_ELF) $(TSM_SIG)
	cargo build --target $(TARGET_TRIPLET) -p shadowfax $(CARGO_FLAGS)

$(TSM_SIG): $(TSM_ELF) | $(BIN_DIR)
	openssl pkeyutl -sign -inkey $(PRIVATE_KEY) -in $< -out $@

$(TSM_ELF):
	 cargo build --target $(TARGET_TRIPLET) -p tsm $(CARGO_FLAGS)

## test: build and run the tests
test: firmware
	cargo test --manifest-path $(TEST_DIR)/Cargo.toml --target $(HOST_TRIPLET)

## generate-keys: generate ed25519 signing keys and DICE initial keys in shadowfax/keys/
generate-keys:
	mkdir -p $(KEYS_DIR)
	openssl genpkey -algorithm ed25519 -out $(PRIVATE_KEY)
	openssl pkey -in $(PRIVATE_KEY) -pubout -out $(PUBLIC_KEY)
	$(PYTHON) scripts/dice_tool.py generate-uds-keys $(DICE_PLATFORM_PRIVATE_KEY) $(DICE_PLATFORM_PUBLIC_KEY)

## qemu-run: runs the script on qemu
qemu-run: firmware
	@set -- $$(fdtget -t x $(FDT_IMAGE) /chosen/shadowfax dice-input); \
	dice_input_addr=$$((0x$$1 << 32 | 0x$$2)); \
	$(QEMU) $(QEMU_FLAGS) -bios $(FW_ELF) \
		-dtb $(FDT_IMAGE) \
		-device loader,file=$(DICE_INPUT),addr=$$dice_input_addr,force-raw=on $(QEMU_DEVICES)

## debug: attach to a gdb server and load $(GDB_COVE_SCRIPT)
debug:
	@set -- $$(fdtget -t x $(FDT_IMAGE) /chosen/opensbi-domains/untrusted-domain next-addr); \
	export BOOT_DOMAIN_ADDRESS=$$(printf '0x%x' $$((0x$$1 << 32 | 0x$$2))); \
	$(GDB) -x $(GDB_SETTINGS_SCRIPT) -x $(GDB_COVE_SCRIPT) $(FW_ELF)

# Ensure bin directory exists
$(BIN_DIR):
	mkdir -p $(BIN_DIR)

## build-info: display build configuration
build-info:
	@echo "Build Configuration:"
	@echo "  HOST_ARCHITECTURE:         $(HOST_ARCHITECTURE)"
	@echo "  HOST_LIBC:                 $(HOST_LIBC)"
	@echo "  HOST_TARGET_TRIPLET:       $(HOST_TRIPLET)"
	@echo "  TARGET_TRIPLET:            $(TARGET_TRIPLET)"
	@echo "  RV_PREFIX:                 $(RV_PREFIX)"
	@echo "  ARCH/ABI:                  $(ARCH)/$(ABI)"
	@echo "  PROFILE:                   $(PROFILE)"
	@echo "  DEBUG:                     $(DEBUG)"
	@echo "  PLATFORM:                  $(PLATFORM)"
	@echo "  FDT_IMAGE:                 $(FDT_IMAGE)"
	@echo "  CFLAGS:                    $(CFLAGS)"
	@echo "  GUEST_CFLAGS:              $(GUEST_CFLAGS)"
	@echo "  GUEST_RAM_SIZE:            $(GUEST_RAM_SIZE)"
	@echo "  ASFLAGS:                   $(ASFLAGS)"
	@echo "  LDFLAGS:                   $(LDFLAGS)"
	@echo "  GUEST_LDFLAGS:             $(GUEST_LDFLAGS)"
	@echo "  RUSTFLAGS:                 $(RUSTFLAGS)"
	@echo "  QEMU_FLAGS:                $(QEMU_FLAGS)"
	@echo "  OPENSBI_VERSION:           $(OPENSBI_VERSION)"
ifeq ($(HOST_LIBC), musl)
	@echo "  LLVM_CONFIG_PATH:          $(LLVM_CONFIG_PATH)"
	@echo "  LIBCLANG_STATIC_PATH:      $(LIBCLANG_STATIC_PATH)"
endif

## clean: remove all build artifacts
clean:
	cargo clean
	$(RM) $(BIN_DIR)/*.bin $(BIN_DIR)/*.elf $(BIN_DIR)/*.signature $(BIN_DIR)/*.sig
	$(RM) $(FDT_IMAGE)
	$(MAKE) -C shadowfax/opensbi clean distclean
	$(MAKE) -C guests clean

## help: display this help message
help:
	@echo "Shadowfax Firmware Build System"
	@echo ""
	@echo "Available targets:"
	@echo ""
	@sed -n 's/^##//p' $(MAKEFILE_LIST) | column -t -s ':' | sed -e 's/^/  /'
	@echo ""
	@echo "Examples:"
	@echo "  make firmware                  # Build debug firmware"
	@echo "  make test                      # Build and test"
	@echo "  make generate-keys             # Generate signing keys"
	@echo "  make debug GDB_COVE_SCRIPT=... # Debug using GDB_COVE_SCRIPT"
