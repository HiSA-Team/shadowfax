# This Makefile contains everything needed to build and run Shadowfax with examples. This Makefile
# is the unique entrypoint for managing Shadowfax build since it detects and the host system and
# sets up reasonable defaults. Variable users may want to override:
#
# - RV_PREFIX:           specify with the path to the target riscv toolchain prefix
# - BOOT_DOMAIN_ADDRESS: specify the address of the untrusted domain which should start the execution
# - PLATFORM:            target platform, this is used for OpenSBI initialization
# - GDB_COVE_SCRIPT:     path to the example to run
#
# Usage:
#   make help # discover available targets
#
# Author: <capassog97@gmail.com>

# Toolchain/Platform
HOST_TRIPLET               := $(shell rustc -vV | grep '^host:' | awk '{print $$2}')
HOST_ARCHITECTURE          := $(shell uname -m)
HOST_LIBC                  := $(shell if ldd --version 2>&1 | grep -q musl; then echo musl; else echo gnu; fi)
OPENSBI_VERSION            := $(shell git -C shadowfax/opensbi describe)
TARGET_TRIPLET             ?= riscv64imac-unknown-none-elf
PROFILE                    ?= debug
RUSTFLAGS                  := -C target-feature=+h

# Platform Params
RV_PREFIX                  ?= riscv64-unknown-linux-$(HOST_LIBC)-
PLATFORM                   ?= generic
BOOT_DOMAIN_ADDRESS        ?= 0x82800000

# Files and Directories
MAKEFILE_SOURCE_DIR        := $(dir $(realpath $(lastword $(MAKEFILE_LIST))))
BIN_DIR                     = bin
TARGET_DIR                  = target/$(TARGET_TRIPLET)/$(PROFILE)
KEYS_DIR                    = shadowfax/keys

TSM_ELF                     = $(TARGET_DIR)/tsm
TSM_SIG                     = $(BIN_DIR)/tsm.bin.signature
PRIVATE_KEY                 = $(KEYS_DIR)/privatekey.pem
PUBLIC_KEY                  = $(KEYS_DIR)/publickey.pem

FW_ELF                      = $(TARGET_DIR)/shadowfax

# Debug variables
GDB                         = $(RV_PREFIX)gdb
GDB_SETTINGS_SCRIPT         := $(MAKEFILE_SOURCE_DIR)scripts/gdb_settings
GDB_COVE_SCRIPT             ?= $(MAKEFILE_SOURCE_DIR)scripts/gdb_covh_get_tsm_info.py

# Needed for OpenSBI
export CROSS_COMPILE        := $(RV_PREFIX)
export RUSTFLAGS
export BOOT_DOMAIN_ADDRESS

ifeq ($(HOST_LIBC), musl)

$(warning Musl system detected. Make sure you provide the libclang.a path in 'scripts/llvm-config.sh' accordingly and provide the path do the build directory in LIBCLANG_STATIC_PATH variable)

ifeq ($(LIBCLANG_STATIC_PATH),)
$(error Missing LIBCLANG_STATIC_PATH environment variable which is required in a musl environment)
endif

export LLVM_CONFIG_PATH     := $(MAKEFILE_SOURCE_DIR)scripts/llvm-config.sh
endif

.PHONY: all clean firmware tsm test generate-keys help

# ensure the bin directory is created
$(shell mkdir -p $(BIN_DIR))

all: firmware build-info

## firmware: build the firmware. It includes building the TSM and signing it
firmware: tsm
	cargo build --target $(TARGET_TRIPLET) -p shadowfax

## tsm: build the TSM. This copies the .elf in bin/ creates a binary and sign it with the keys in keys/
tsm: $(TSM_SIG)

$(TSM_SIG): $(TSM_ELF)
	openssl dgst -sha256 -sign $(PRIVATE_KEY) -out $@ $<

$(TSM_ELF):
	 cargo build --target $(TARGET_TRIPLET) -p tsm

## test: build and run the tests
test: firmware
	cargo test --manifest-path test/Cargo.toml --target $(HOST_TRIPLET)

## generate-keys: generate a couple of RSA keys 2048 bit in shadowfax-core/keys/
generate-keys:
	mkdir -p $(KEYS_DIR)
	openssl genrsa -out $(PRIVATE_KEY) 2048
	openssl rsa -in $(PRIVATE_KEY) -RSAPublicKey_out -outform PEM -out $(PUBLIC_KEY)

## debug: attach to a gdb server and load $(GDB_COVE_SCRIPT)
debug:
	$(GDB) -x $(GDB_SETTINGS_SCRIPT) -x $(GDB_COVE_SCRIPT) $(FW_ELF)

## build-info: display build configuration
build-info:
	@echo "Build Configuration:"
	@echo "  HOST_ARCHITECTURE:         $(HOST_ARCHITECTURE)"
	@echo "  HOST_LIBC:                 $(HOST_LIBC)"
	@echo "  HOST_TARGET_TRIPLET:       $(HOST_TRIPLET)"
	@echo "  TARGET_TRIPLET:            $(TARGET_TRIPLET)"
	@echo "  CROSS_COMPILE:             $(CROSS_COMPILE)"
	@echo "  PROFILE:                   $(PROFILE)"
	@echo "  PLATFORM:                  $(PLATFORM)"
	@echo "  RUSTFLAGS:                 $(RUSTFLAGS)"
	@echo "  OPENSBI_VERSION:           $(OPENSBI_VERSION)"
	@echo "  BOOT_DOMAIN_ADDRESS:       $(BOOT_DOMAIN_ADDRESS)"
ifeq ($(HOST_LIBC), musl)
	@echo "  LLVM_CONFIG_PATH:          $(LLVM_CONFIG_PATH)"
	@echo "  LIBCLANG_STATIC_PATH:      $(LIBCLANG_STATIC_PATH)"
endif

## clean: remove all build artifacts
clean:
	cargo clean
	$(RM) $(BIN_DIR)/*.bin $(BIN_DIR)/*.elf $(BIN_DIR)/*.signature $(BIN_DIR)/*.sig
	$(MAKE) -C shadowfax/opensbi clean

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
