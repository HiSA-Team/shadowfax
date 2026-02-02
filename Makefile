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
#   make qemu-run # runs the system on qemu (DEBUG=1 to start gdb server and wait)
#
# Author: <capassog97@gmail.com>

# Toolchain/Platform
PYTHON                     := python
HOST_TRIPLET               := $(shell rustc -vV | grep '^host:' | awk '{print $$2}')
HOST_ARCHITECTURE          := $(shell uname -m)
HOST_LIBC                  := $(shell if ldd --version 2>&1 | grep -q musl; then echo musl; else echo gnu; fi)
OPENSBI_VERSION            := $(shell git -C shadowfax/opensbi describe)
TARGET_TRIPLET             ?= riscv64imac-unknown-none-elf
PROFILE                    ?= debug
RUSTFLAGS                  := -C target-feature=+h
QEMU                       := qemu-system-riscv64
QEMU_FLAGS                 := -M virt -m 64M -smp 1 -nographic -monitor unix:/tmp/shadowfax-qemu-monitor,server,nowait
ifeq ($(DEBUG), 1)
QEMU_FLAGS                 +=  -s -S
endif

# Platform Params
PLATFORM                   ?= generic
BOOT_DOMAIN_ADDRESS        ?= 0x82800000

# RISC-V Toolchain
RV_PREFIX                  ?= riscv64-unknown-linux-$(HOST_LIBC)-
OBJCOPY                    := $(RV_PREFIX)objcopy

# Files and Directories
MAKEFILE_SOURCE_DIR        := $(dir $(realpath $(lastword $(MAKEFILE_LIST))))
BIN_DIR                     = bin
TARGET_DIR                  = target/$(TARGET_TRIPLET)/$(PROFILE)
KEYS_DIR                    = shadowfax/keys
TEST_DIR                    = test/functional/

FW_ELF                      = $(TARGET_DIR)/shadowfax
FW_BIN                      = $(BIN_DIR)/shadowfax.bin
TSM_ELF                     = $(TARGET_DIR)/tsm
TSM_SIG                     = $(BIN_DIR)/tsm.bin.signature

# Keys and Dice files
DICE_INPUT                  = $(BIN_DIR)/shadowfax.dice.bin
PRIVATE_KEY                 = $(KEYS_DIR)/privatekey.pem
PUBLIC_KEY                  = $(KEYS_DIR)/publickey.pem
DICE_PLATFORM_PUBLIC_KEY    = $(KEYS_DIR)/root_of_trust_pub.bin
DICE_PLATFORM_PRIVATE_KEY   = $(KEYS_DIR)/root_of_trust_priv.bin

# Debug variables for QEMU
GDB                         = $(RV_PREFIX)gdb
GDB_SETTINGS_SCRIPT         := test/debug/gdbinit
GDB_COVE_SCRIPT             ?= test/debug/gdb_covh_get_tsm_info.py

# Needed for OpenSBI
export RV_PREFIX

# Needed to avoid passing manually to Cargo
export RUSTFLAGS

# Needed by Python GDB process
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

## all: build tsm, firmware and attestation payload
all: $(DICE_INPUT) build-info

## firmware: builds the firmware alongisde TSM elf and its signature
firmware: $(DICE_INPUT)

## tsm: build the TSM and signs it
tsm: $(TSM_SIG)

# create attestation input (CDI_ID and Certificate) according to DICE specification
$(DICE_INPUT): $(FW_BIN)
	$(PYTHON) scripts/dice_tool.py generate-platform-token \
		--uds-private-key $(DICE_PLATFORM_PRIVATE_KEY) \
		--uds-public-key $(DICE_PLATFORM_PUBLIC_KEY) \
		$< $@

$(FW_BIN): $(FW_ELF)
	$(OBJCOPY) -O binary $< $@

$(FW_ELF): $(TSM_ELF) $(TSM_SIG)
	cargo build --target $(TARGET_TRIPLET) -p shadowfax

$(TSM_SIG): $(TSM_ELF)
	openssl pkeyutl -sign -inkey $(PRIVATE_KEY) -in $< -out $@

$(TSM_ELF):
	 cargo build --target $(TARGET_TRIPLET) -p tsm

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
	$(QEMU) $(QEMU_FLAGS) -dtb $(BIN_DIR)/device-tree.dtb -bios $(FW_ELF) \
		-device loader,file=$(DICE_INPUT),addr=0x82000000,force-raw=on

## debug: attach to a gdb server and load $(GDB_COVE_SCRIPT)
debug:
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
