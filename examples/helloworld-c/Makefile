include ../common/config.mk

main: baremetal.ld main.o init.o
	$(LD) $(LDFLAGS) -static -o $@ -T $^

main.o: main.c
	$(CC) $(CFLAGS) -nostdlib -ffreestanding -c -o $@ $<

init.o: ../common/init.S
	$(AS) $(ASFLAGS) -o $@ $<

run: main
	@echo "Press (ctrl + a) and then x to quit"
	qemu-system-riscv64 -nographic -machine virt -bios $<

clean:
	rm -f main *.o
