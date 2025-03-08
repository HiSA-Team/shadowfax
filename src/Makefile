include ../config.mk

CFLAGS += -Iinclude/
LIBSBI_PATH = ../lib64/lp64/libsbi.a

shadowfax: init.ld init.o
	$(LD) $(LDFLAGS) -static -o $@ -T $^ $(LIBSBI_PATH)

init.o: init.S
	$(CC) $(CFLAGS) -c -o $@ $<

run: shadowfax
	@echo "Press (ctrl + a) and then x to quit"
	qemu-system-riscv64 -nographic -machine virt -bios $<

clean:
	rm -f shadowfax *.o
