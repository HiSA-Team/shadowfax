CC = riscv64-linux-musl-gcc
LD = riscv64-linux-musl-ld

CFLAGS = -c -Wall -Wextra -Werror
LDFLAGS = 

main: baremetal.ld main.o
	$(LD) $(LDFLAGS) -o $@ -T $^

main.o: main.s
	$(CC) $(CFLAGS) -march=rv64gh -o $@ $<

run: main
	echo "Press (ctrl + a) and then x to quit"
	qemu-system-riscv64 -nographic -machine virt -bios $<

clean:
	rm -f main *.o



