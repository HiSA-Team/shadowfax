include ../../config.mk

main: baremetal.ld main.o start.o
	$(LD) $(LDFLAGS) -o $@ -T $^

main.o: main.c
	$(CC) $(CFLAGS) -ffreestanding -c -march=$(ARCH) -o $@ $<

start.o: start.s
	$(AS) -march=$(ARCH) -o $@ $<

run: main
	@echo "Press (ctrl + a) and then x to quit"
	qemu-system-riscv64 -nographic -machine virt -bios $<

clean:
	rm -f main *.o



