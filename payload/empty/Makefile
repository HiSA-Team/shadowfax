include ../../config.mk

.PHONY: all clean
all: empty.elf

%.o: %.S
	$(AS) $(ASFLAGS) -c -o $@ $<

%.elf: %.o
	$(LD) $(LDFLAGS) -Ttext=0x0 -o $@ $^

clean:
	rm -f empty.elf
