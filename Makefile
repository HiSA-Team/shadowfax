include config.mk

LIBSBI_PATH = lib64/lp64/opensbi/generic/lib/libplatsbi.a
CFLAGS += -Iinclude/
QEMUFLAGS = -nographic -machine virt -smp 1 -m 64M

ifeq ($(DEBUG), 1)
	CFLAGS += -g
	LDFLAGS += -g
	QEMUFLAGS += -s -S
endif

kernel.bin: kernel.o
	$(LD) -T kernel.ld --no-dynamic-linker -static -nostdlib -o kernel.elf $<
	$(OBJCOPY) -O binary kernel.elf $@

kernel.o: kernel.S
	$(CC) $(CFLAGS) -c -o $@ $<

gdb:
	gdb shadowfax -ex "set architecture riscv:rv64" \
		-ex "target remote localhost:1234" \
		-ex "set disassemble-next-line on"

clean:
	rm -f shadowfax *.o *.elf
