ROOT       := ..
RV_PREFIX ?= riscv64-unknown-elf-
include $(ROOT)/config.mk

# Identify all C source files and their corresponding ELF names
C_SRCS  = $(wildcard *.c)
BIN     = $(C_SRCS:.c=.bin)
ELF    = $(C_SRCS:.c=.out)

.PHONY: all clean

all: $(BIN) $(ELF)

%.bin: %.out
	$(OBJCOPY) -O binary $^ $@

%.out: %.o vmstartup.o
	$(CC) $(LDFLAGS) -Tlinker.ld $^ -o $@

vmstartup.o: vmstartup.S
	$(AS) $(ASFLAGS) -c $< -o $@

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f *.o *.out *.bin
