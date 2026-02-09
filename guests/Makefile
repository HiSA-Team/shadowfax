# Toolchain definition
RV_PREFIX = riscv64-unknown-elf
CC      = $(RV_PREFIX)-gcc

# Flags
CFLAGS  = -Wall -nostdlib -nostartfiles -O0 -g
LDFLAGS = -Tlinker.ld

# Shared files
AS_SRC  = vmstartup.S
AS_OBJ  = vmstartup.o

# Identify all C source files and their corresponding ELF names
C_SRCS  = $(wildcard *.c)
ELFS    = $(C_SRCS:.c=.out)

.PHONY: all clean

# Default target builds everything
all: $(ELFS)

# 1. Rule to link the ELF file (Combines the specific C file with shared startup)
# This uses the static startup object as a dependency
%.out: %.o $(AS_OBJ)
	$(CC) $(CFLAGS) $(LDFLAGS) $^ -o $@

# 2. Rule for shared assembly file
$(AS_OBJ): $(AS_SRC)
	$(CC) $(CFLAGS) -c $< -o $@

# 3. General rule for C files
%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f *.o *.out *.bin
