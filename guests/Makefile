# Toolchain definition
RV_PREFIX = riscv64-unknown-elf
CC      = $(RV_PREFIX)-gcc
OBJCOPY = $(RV_PREFIX)-objcopy

# Flags
CFLAGS  = -Wall -nostdlib -nostartfiles -O0 -g
LDFLAGS = -Wl,-Ttext=0x1000 \
          -Wl,--defsym=__stack_top=0x39F0 \
          -Wl,--defsym=_start=0x1000

# Shared files
AS_SRC  = vmstartup.S
AS_OBJ  = vmstartup.o

# Identify all C source files and their corresponding binary names
C_SRCS  = $(wildcard *.c)
BINS    = $(C_SRCS:.c=.bin)
ELFS    = $(C_SRCS:.c=.out)

.PHONY: all clean

# Default target builds everything
all: $(BINS)

# 1. Rule to create .bin from .out (ELF)
%.bin: %.out
	$(OBJCOPY) -O binary $< $@

# 2. Rule to link the ELF file (Combines the specific C file with shared startup)
# This uses the static startup object as a dependency
%.out: %.o $(AS_OBJ)
	$(CC) $(CFLAGS) $(LDFLAGS) $^ -o $@

# 3. Rule for shared assembly file
$(AS_OBJ): $(AS_SRC)
	$(CC) $(CFLAGS) -c $< -o $@

# 4. General rule for C files
%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f *.o *.out *.bin
