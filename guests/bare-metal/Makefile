ROOT       := ../..
RV_PREFIX ?= riscv64-unknown-elf-
include $(ROOT)/config.mk

# Identify all C source files and their corresponding ELF names
C_SRCS  = $(wildcard *.c)
BIN     = $(C_SRCS:.c=.bin)
ELF    = $(C_SRCS:.c=.out)
LIB_OBJ = $(GUEST_BUILD_DIR)/baremetal.o $(GUEST_BUILD_DIR)/baremetal_elf.o

.PHONY: all clean

all: $(GUEST_LIB) $(BIN) $(ELF)

$(GUEST_BUILD_DIR):
	mkdir -p $@

$(GUEST_BUILD_DIR)/baremetal.o: lib/baremetal.c include/baremetal.h | $(GUEST_BUILD_DIR)
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) -c $< -o $@

$(GUEST_BUILD_DIR)/baremetal_elf.o: lib/baremetal_elf.c \
		include/baremetal.h include/baremetal_elf.h | $(GUEST_BUILD_DIR)
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) -c $< -o $@

$(GUEST_LIB): $(LIB_OBJ)
	$(AR) rcs $@ $^

%.bin: %.out
	$(OBJCOPY) -O binary $^ $@

%.out: %.o vmstartup.o $(GUEST_LIB)
	$(CC) $(LDFLAGS) -Tlinker.ld $^ -o $@

vmstartup.o: vmstartup.S
	$(AS) $(ASFLAGS) -c $< -o $@

%.o: %.c
	$(CC) $(CFLAGS) $(GUEST_CFLAGS) -c $< -o $@

clean:
	$(RM) -r $(GUEST_BUILD_DIR)
	$(RM) *.o *.out *.bin
