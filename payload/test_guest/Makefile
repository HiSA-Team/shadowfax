include ../common/config.mk

CFLAGS += -O0 -g
LDFLAGS += -nostdlib -T kernel.ld

.PHONY: all clean

all: kernel.elf kernel.bin

kernel.elf: ecall.S kernel.ld
	$(CC)   $(CFLAGS) -c ecall.S -o payload.o
	$(LD)   payload.o $(LDFLAGS) -o kernel.elf

# (optional) produce a flat binary
kernel.bin: kernel.elf
	$(OBJCOPY) -O binary kernel.elf kernel.bin

clean:
	rm -f *.o *.bin *.elf
