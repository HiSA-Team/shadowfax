include config.mk

LIBSBI_PATH = lib64/lp64/opensbi/generic/lib/libplatsbi.a
CFLAGS += -Iinclude/
QEMUFLAGS = -nographic -machine virt -smp 1 -m 64M

ifeq ($(DEBUG), 1)
	CFLAGS += -g
	LDFLAGS += -g
	QEMUFLAGS += -s -S
endif

shadowfax: init.ld init.o
	$(LD) $(LDFLAGS) -static -o $@ -T $^ $(LIBSBI_PATH)

init.o: init.S
	$(CC) $(CFLAGS) -c -o $@ $<

run: shadowfax
	@echo "Press (ctrl + a) and then x to quit"
	qemu-system-riscv64 $(QEMUFLAGS) -bios $<

gdb: shadowfax
	gdb shadowfax -ex "set architecture riscv:rv64" \
		-ex "target remote localhost:1234" \
		-ex "set disassemble-next-line on" \
		-ex "break _start"

clean:
	rm -f shadowfax *.o
