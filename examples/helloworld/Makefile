include ../../config.mk

main: baremetal.ld main.o
	$(LD) $(LDFLAGS) -o $@ -T $^

main.o: main.s
	$(CC) $(CFLAGS) -c -march=rv64gh -o $@ $<

run: main
	echo "Press (ctrl + a) and then x to quit"
	qemu-system-riscv64 -nographic -machine virt -bios $<

clean:
	rm -f main *.o



