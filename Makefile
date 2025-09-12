TARGET ?= riscv64imac-unknown-none-elf
PROFILE ?= debug

OBJCOPY = $(CROSS_COMPILE)objcopy

# Directories
BIN_DIR = bin
KEYS_DIR = shadowfax-core/keys
TARGET_DIR = target/$(TARGET)/$(PROFILE)

# Files
TSM_ELF = $(BIN_DIR)/tsm.elf
TSM_BIN = $(BIN_DIR)/tsm.bin
TSM_SIG = $(BIN_DIR)/tsm.bin.signature
PRIVATE_KEY = $(KEYS_DIR)/privatekey.pem
PUBLIC_KEY = $(KEYS_DIR)/publickey.pem
PUBLIC_KEY_DER = $(KEYS_DIR)/publickey.der

.PHONY: all clean firmware tsm hypervisor test generate-keys help info

ifeq ($(OPENSBI_PATH),)
$(error OPENSBI_PATH not set. Run: source environment.sh <opensbi-path>)
endif

# ensure the bin directory is created
$(shell mkdir -p $(BIN_DIR))

all: firmware info

## firmware: build the firmware. It includes building the TSM and signing it
firmware: tsm
	cargo build --target $(TARGET) -p shadowfax-core

## tsm: build the TSM. This copies the .elf in bin/ creates a binary and sign it with the keys in keys/
tsm: $(TSM_SIG)

$(TSM_SIG): $(TSM_BIN)
	openssl dgst -sha256 -sign $(PRIVATE_KEY) -out $@ $<

$(TSM_BIN): $(TSM_ELF)
	$(OBJCOPY) -O binary $(TSM_ELF) $(TSM_BIN)

$(TSM_ELF):
	cargo build --target $(TARGET) -p shadowfax-tsm
	cp $(TARGET_DIR)/shadowfax-tsm $@

## hypervisor: build the Hypervisor
hypervisor:
	cargo build --target $(TARGET) -p hypervisor

## test: builds and run the tests
test: firmware
	cargo test -p shadowfax-test

## generate-keys: generates a couple of RSA keys 2048 bit in shadowfax-core/keys/
generate-keys:
	mkdir -p $(KEYS_DIR)
	openssl genrsa -out $(PRIVATE_KEY) 2048
	openssl rsa -in $(PRIVATE_KEY) -outform PEM -pubout -out $(PUBLIC_KEY)
	openssl rsa -pubin -inform PEM -in $(PUBLIC_KEY) -outform DER -out $(PUBLIC_KEY_DER)

## info: display build configuration
info:
	@echo "Build Configuration:"
	@echo "  TARGET:        $(TARGET)"
	@echo "  PROFILE:       $(PROFILE)"
	@echo "  OPENSBI_PATH:  $(OPENSBI_PATH)"
	@echo "  PLATFORM:      $(PLATFORM)"
	@echo "  CROSS_COMPILE: $(CROSS_COMPILE)"
	@echo "  ARCHITECTURE:  $(ARCHITECTURE)"
	@echo "  LIBC_PREFIX:   $(LIBC_PREFIX)"

## clean: removes all build artifacts
clean:
	cargo clean
	$(RM) $(BIN_DIR)/*.bin $(BIN_DIR)/*.elf $(BIN_DIR)/*.signature $(BIN_DIR)/*.sig || true

## help: display this help message
help:
	@echo "Shadowfax Firmware Build System"
	@echo ""
	@echo "Prerequisites:"
	@echo "  source environment.sh <opensbi-path>"
	@echo ""
	@echo "Available targets:"
	@echo ""
	@sed -n 's/^##//p' $(MAKEFILE_LIST) | column -t -s ':' | sed -e 's/^/  /'
	@echo ""
	@echo "Examples:"
	@echo "  make firmware          # Build debug firmware"
	@echo "  make test              # Build and test"
	@echo "  make generate-keys     # Generate signing keys"
