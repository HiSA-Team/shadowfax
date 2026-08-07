# Toolchain definition
RV_PREFIX ?= riscv64-unknown-elf-
CC         = $(RV_PREFIX)gcc
AS         = $(RV_PREFIX)as
OBJCOPY    = $(RV_PREFIX)objcopy

# Flags
ARCH    = -march=rv64imac -mabi=lp64
CFLAGS  = -Wall -nostdlib $(ARCH)
ASFLAGS = $(ARCH)
LDFLAGS = -Tlinker.ld -ffreestanding -nostartfiles

ifeq ($(DEBUG), 1)
CFLAGS += -O0 -g
else
CFLAGS += -O1
endif

# Identify all C source files and their corresponding ELF names
C_SRCS  = $(wildcard *.c)
BIN     = $(C_SRCS:.c=.bin)
ELF    = $(C_SRCS:.c=.out)

.PHONY: all clean

all: $(BIN) $(ELF)

%.bin: %.out
	$(OBJCOPY) -O binary $^ $@

%.out: %.o vmstartup.o
	$(CC) $(LDFLAGS) $^ -o $@

vmstartup.o: vmstartup.S
	$(AS) $(ASFLAGS) -c $< -o $@

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f *.o *.out *.bin
