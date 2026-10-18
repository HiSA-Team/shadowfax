# This Makefile contains everything needed to build and run Shadowfax with examples. This Makefile
# is the unique entrypoint for managing Shadowfax build since it detects and the host system and
# sets up reasonable defaults. Variable users may want to override:
#
# - RV_PREFIX:           specify with the path to the target riscv toolchain prefix
# - PLATFORM:            target platform, this is used for OpenSBI initialization
#
# This Makefile stages artifacts; it does not run them. Launch the platform
# through a launcher under test/, for example `make -C test/standalone-tvm-launcher run`.
#
# Usage:
#   make help # discover available targets
#
# Author: Giuseppe Capasso <capassog97@gmail.com>

RUSTFLAGS         ?= -C target-feature=+h
HOST_ARCHITECTURE := $(shell uname -m)
HOST_TRIPLET      := $(shell rustc -vV | awk '/^host:/ { print $$2 }')
HOST_LIBC         := $(shell if ldd --version 2>&1 | grep -q musl; then echo musl; else echo gnu; fi)
RV_PREFIX         ?= riscv64-unknown-linux-$(HOST_LIBC)-

include config.mk

# Every launcher under test/ owns a Makefile that includes launcher.mk; each
# one knows how to remove its own build directory.
LAUNCHER_DIRS              := $(patsubst %/,%,$(sort $(dir $(wildcard test/*/Makefile test/*/*/Makefile))))
LAUNCHER_CLEAN             := $(addsuffix .clean,$(LAUNCHER_DIRS))

OPENSBI_VERSION            := $(shell git -C shadowfax/opensbi describe)
OPENSBI_PATCH              := shadowfax/opensbi-sbi-domain-change-active.diff

# Files and Directories
BIN_DIR                     = target/artifacts
TARGET_DIR                  = $(CURDIR)/target/$(TARGET_TRIPLET)/$(PROFILE)
KEYS_DIR                    = shadowfax/keys
CARGO_FLAGS                 =

FW_ELF                      = $(TARGET_DIR)/shadowfax
FW_BIN                      = $(BIN_DIR)/shadowfax.bin
TSM_ELF                     = $(TARGET_DIR)/tsm
TSM_IMAGE                   = $(BIN_DIR)/tsm.elf
TSM_SIG                     = $(BIN_DIR)/tsm.bin.signature
TSM_RUSTFLAGS               = $(RUSTFLAGS) -C relocation-model=pic -C link-arg=-pie
TSM_STANDALONE_DIR          = $(CURDIR)/target/standalone
TSM_STANDALONE              = $(TSM_STANDALONE_DIR)/$(TARGET_TRIPLET)/$(PROFILE)/tsm

# Keys and Dice files
DICE_INPUT                  = $(BIN_DIR)/shadowfax.dice.bin
FDT_SOURCE                  = platform/$(PLATFORM)/device-tree.dts
PRIVATE_KEY                 = $(KEYS_DIR)/privatekey.pem
PUBLIC_KEY                  = $(KEYS_DIR)/publickey.pem
DICE_PLATFORM_PUBLIC_KEY    = $(KEYS_DIR)/root_of_trust_pub.bin
DICE_PLATFORM_PRIVATE_KEY   = $(KEYS_DIR)/root_of_trust_priv.bin

# Debug variables for QEMU

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

.PHONY: all clean firmware tsm tsm-standalone test generate-keys guests help opensbi-patch \
	$(LAUNCHER_CLEAN)

## all: build tsm, firmware, attestation payload, and platform DTB
all: guests firmware build-info

## guests: build bare-metal guests in guests/bare-metal/
guests:
	$(MAKE) -C guests/

## firmware: builds the firmware, TSM, DICE input, and platform DTB
firmware: opensbi-patch $(PRIVATE_KEY) $(DICE_PLATFORM_PRIVATE_KEY) \
	$(DICE_PLATFORM_PUBLIC_KEY) $(TSM_SIG) $(DICE_INPUT) $(FDT_IMAGE)

opensbi-patch: $(OPENSBI_PATCH)
	@patch=$$(realpath $<); \
	git -C shadowfax/opensbi rev-parse --is-inside-work-tree >/dev/null 2>&1 || { \
		echo "OpenSBI submodule is not initialized; run: git submodule update --init shadowfax/opensbi" >&2; \
		exit 1; \
	}; \
	if git -C shadowfax/opensbi apply --reverse --check "$$patch" 2>/dev/null; then \
		:; \
	elif git -C shadowfax/opensbi apply --check "$$patch"; then \
		git -C shadowfax/opensbi apply "$$patch"; \
	else \
		echo "OpenSBI patch cannot be applied cleanly: $(OPENSBI_PATCH)" >&2; \
		exit 1; \
	fi

## tsm: build the TSM and signs it
tsm: $(TSM_SIG)

## tsm-standalone: build the TSM as a self-booting QEMU payload
# Built into its own target directory so the `standalone` feature can never
# reach the signed image that `firmware` stages.
tsm-standalone:
	RUSTFLAGS='$(TSM_RUSTFLAGS)' cargo -Z build-std=core,alloc build \
		--target $(TARGET_TRIPLET) --target-dir $(TSM_STANDALONE_DIR) \
		-p tsm --features standalone
	@echo "standalone TSM: $(TSM_STANDALONE)"

# Shadowfax: ELF -> flat image -> DICE measurement input.
# Cargo emits make-compatible dep-info beside each binary; including it below
# gives these rules the real Rust source list, so nothing needs `make -B`.
$(FW_ELF): Cargo.lock shadowfax/Cargo.toml shadowfax/memory.x
	cargo build --target $(TARGET_TRIPLET) -p shadowfax $(CARGO_FLAGS)

$(FW_BIN): $(FW_ELF) | $(BIN_DIR)
	$(OBJCOPY) -O binary $< $@

$(DICE_INPUT): $(FW_BIN) $(DICE_PLATFORM_PRIVATE_KEY) $(DICE_PLATFORM_PUBLIC_KEY) | $(BIN_DIR)
	$(PYTHON3) scripts/dice_tool.py generate-platform-token \
		--uds-private-key $(DICE_PLATFORM_PRIVATE_KEY) \
		--uds-public-key $(DICE_PLATFORM_PUBLIC_KEY) \
		$< $@

# TSM: position-independent ELF -> stripped image -> detached signature.
$(TSM_ELF): Cargo.lock tsm/Cargo.toml tsm/memory.x
	RUSTFLAGS='$(TSM_RUSTFLAGS)' cargo -Z build-std=core,alloc build \
		--target $(TARGET_TRIPLET) -p tsm $(CARGO_FLAGS)

$(TSM_IMAGE): $(TSM_ELF) | $(BIN_DIR)
	$(OBJCOPY) --strip-all $< $@

$(TSM_SIG): $(TSM_IMAGE) $(PRIVATE_KEY)
	openssl pkeyutl -sign -inkey $(PRIVATE_KEY) -in $< -out $@

# Platform: layout.py compiles the tree, writes the staged artifact sizes into
# it, checks the memory map, and emits layout.{mk,sh,h,json} beside the blob.
$(FDT_IMAGE): $(FDT_SOURCE) $(TSM_IMAGE) $(TSM_SIG) $(LAYOUT_PY)
	$(PYTHON3) $(LAYOUT_PY) $(FDT_SOURCE) $(LAYOUT_DIR) --artifacts $(BIN_DIR)

-include $(FW_ELF).d $(TSM_ELF).d $(LAYOUT_DIR)/layout.d

## test: build the firmware and run every QEMU test
test: firmware guests
	scripts/run-tests.sh

## generate-keys: generate ed25519 signing keys and DICE initial keys in shadowfax/keys/
generate-keys:
	mkdir -p $(KEYS_DIR)
	openssl genpkey -algorithm ed25519 -out $(PRIVATE_KEY)
	openssl pkey -in $(PRIVATE_KEY) -pubout -out $(PUBLIC_KEY)
	$(PYTHON3) scripts/dice_tool.py generate-uds-keys $(DICE_PLATFORM_PRIVATE_KEY) $(DICE_PLATFORM_PUBLIC_KEY)

$(BIN_DIR):
	mkdir -p $(BIN_DIR)

# The keys are development material and are gitignored, so a fresh clone has
# none. Without these rules make reports "No rule to make target" against a
# path the reader has no reason to recognise -- and only after cargo has spent
# a minute building the TSM it is about to fail to sign.
$(PRIVATE_KEY) $(PUBLIC_KEY) $(DICE_PLATFORM_PRIVATE_KEY) $(DICE_PLATFORM_PUBLIC_KEY):
	@echo "missing signing key: $@" >&2
	@echo "run: make generate-keys" >&2
	@exit 1

## build-info: display build configuration
build-info:
	@echo "Build Configuration:"
	@echo "  HOST_ARCHITECTURE:         $(HOST_ARCHITECTURE)"
	@echo "  HOST_LIBC:                 $(HOST_LIBC)"
	@echo "  HOST_TARGET_TRIPLET:       $(HOST_TRIPLET)"
	@echo "  TARGET_TRIPLET:            $(TARGET_TRIPLET)"
	@echo "  RV_PREFIX:                 $(RV_PREFIX)"
	@echo "  PYTHON3:                   $(PYTHON3)"
	@echo "  ARCH/ABI:                  $(ARCH)/$(ABI)"
	@echo "  PROFILE:                   $(PROFILE)"
	@echo "  DEBUG:                     $(DEBUG)"
	@echo "  PLATFORM:                  $(PLATFORM)"
	@echo "  FDT_IMAGE:                 $(FDT_IMAGE)"
	@echo "  CFLAGS:                    $(CFLAGS)"
	@echo "  GUEST_CFLAGS:              $(GUEST_CFLAGS)"
	@echo "  GUEST_RAM_SIZE:            $(GUEST_RAM_SIZE)"
	@echo "  TSM_RUSTFLAGS:             $(TSM_RUSTFLAGS)"
	@echo "  ASFLAGS:                   $(ASFLAGS)"
	@echo "  LDFLAGS:                   $(LDFLAGS)"
	@echo "  GUEST_LDFLAGS:             $(GUEST_LDFLAGS)"
	@echo "  RUSTFLAGS:                 $(RUSTFLAGS)"
	@echo "  OPENSBI_VERSION:           $(OPENSBI_VERSION)"
ifeq ($(HOST_LIBC), musl)
	@echo "  LLVM_CONFIG_PATH:          $(LLVM_CONFIG_PATH)"
	@echo "  LIBCLANG_STATIC_PATH:      $(LIBCLANG_STATIC_PATH)"
endif

$(LAUNCHER_CLEAN):
	$(MAKE) -C $(@:.clean=) clean

## clean: remove all build artifacts, including every launcher under test/
clean: $(LAUNCHER_CLEAN)
	cargo clean
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
	@echo "  make -C test/standalone-tvm-launcher run  # Run a launcher"
