include ../common/config.mk

ASFLAGS += -O0 -g
LDFLAGS += -static -nostdlib -T kernel.ld

.PHONY: all clean

all: ecall.elf empty.elf

ecall.elf: ecall.S kernel.ld
	$(CC) $(ASFLAGS) -c $< -o ecall.o
	$(LD) ecall.o $(LDFLAGS) -o $@

empty.elf: empty.S kernel.ld
	$(CC) $(ASFLAGS) -c $< -o empty.o
	$(LD) empty.o $(LDFLAGS) -o $@

clean:
	rm -f *.o *.elf
