# Toolchain
HOST_TRIPLET      := $(shell rustc -vV | grep '^host:' | awk '{print $$2}')
OPENSBI_VERSION   := $(shell git -C shadowfax/opensbi describe)
TARGET_TRIPLET    ?= riscv64imac-unknown-none-elf
PROFILE           ?= debug
HOST_ARCHITECTURE ?= $(shell uname -m)

# OpenSBI Params
CROSS_COMPILE     ?= riscv64-unknown-linux-${LIBC_PREFIX}-
PLATFORM          ?= generic

# General Directories
BIN_DIR          = bin
KEYS_DIR         = shadowfax/keys
TARGET_DIR       = target/$(TARGET_TRIPLET)/$(PROFILE)

# TSM Files
TSM_ELF          = $(TARGET_DIR)/tsm
TSM_SIG          = $(BIN_DIR)/tsm.bin.signature
PRIVATE_KEY      = $(KEYS_DIR)/privatekey.pem
PUBLIC_KEY       = $(KEYS_DIR)/publickey.pem

.PHONY: all clean firmware tsm test generate-keys help

# ensure the bin directory is created
$(shell mkdir -p $(BIN_DIR))

all: firmware build-info

## firmware: build the firmware. It includes building the TSM and signing it
firmware: tsm
	 CROSS_COMPILE=$(CROSS_COMPILE) cargo build --target $(TARGET_TRIPLET) -p shadowfax

## tsm: build the TSM. This copies the .elf in bin/ creates a binary and sign it with the keys in keys/
tsm: $(TSM_SIG)

$(TSM_SIG): $(TSM_ELF)
	openssl dgst -sha256 -sign $(PRIVATE_KEY) -out $@ $<

$(TSM_ELF):
	 cargo build --target $(TARGET_TRIPLET) -p tsm

## test: builds and run the tests
test: firmware
	cargo test --manifest-path test/Cargo.toml --target $(HOST_TRIPLET)

## generate-keys: generates a couple of RSA keys 2048 bit in shadowfax-core/keys/
generate-keys:
	mkdir -p $(KEYS_DIR)
	openssl genrsa -out $(PRIVATE_KEY) 2048
	openssl rsa -in $(PRIVATE_KEY) -RSAPublicKey_out -outform PEM -out $(PUBLIC_KEY)

## info: display build configuration
build-info:
	@echo "Build Configuration:"
	@echo "  HOST_ARCHITECTURE:         $(HOST_ARCHITECTURE)"
	@echo "  HOST_LIBC_PREFIX:          $(LIBC_PREFIX)"
	@echo "  HOST_TARGET_TRIPLET:       $(HOST_TRIPLET)"
	@echo "  TARGET_TRIPLET:            $(TARGET_TRIPLET)"
	@echo "  CROSS_COMPILE:             $(CROSS_COMPILE)"
	@echo "  PROFILE:                   $(PROFILE)"
	@echo "  PLATFORM:                  $(PLATFORM)"
	@echo "  RUSTFLAGS:                 $(RUSTFLAGS)"
	@echo "  OPENSBI_VERSION:           $(OPENSBI_VERSION)"
	@echo "  BOOT_DOMAIN_ADDRESS:       $(BOOT_DOMAIN_ADDRESS)"

## clean: removes all build artifacts
clean:
	cargo clean
	$(RM) $(BIN_DIR)/*.bin $(BIN_DIR)/*.elf $(BIN_DIR)/*.signature $(BIN_DIR)/*.sig
	$(MAKE) -C shadowfax/opensbi clean

## help: display this help message
help:
	@echo "Shadowfax Firmware Build System"
	@echo ""
	@echo "Prerequisites:"
	@echo "  source environment.sh"
	@echo ""
	@echo "Available targets:"
	@echo ""
	@sed -n 's/^##//p' $(MAKEFILE_LIST) | column -t -s ':' | sed -e 's/^/  /'
	@echo ""
	@echo "Examples:"
	@echo "  make firmware          # Build debug firmware"
	@echo "  make test              # Build and test"
	@echo "  make generate-keys     # Generate signing keys"
