include ../../../config.mk

# =========================
#  Sources & Objects
# =========================
ALL_SRCS        := $(wildcard *.c) $(wildcard *.S)
ALL_OBJS        := $(patsubst %.c,%.o,$(ALL_SRCS))   $(patsubst %.S,%.o,$(ALL_SRCS))
ALL_ELFS        := $(patsubst %.c,%.elf,$(ALL_SRCS)) $(patsubst %.S,%.elf,$(ALL_SRCS))
ALL_BINS        := $(patsubst %.c,%.bin,$(ALL_SRCS)) $(patsubst %.S,%.bin,$(ALL_SRCS))

.PHONY: all clean
all: $(ALL_BINS)

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

%.o: %.S
	$(AS) $(ASFLAGS) -c -o $@ $<

%.elf: kernel.ld %.o
	$(LD) $(LDFLAGS) -o $@ -T $^

%.bin: %.elf
	$(OBJCOPY) -O binary $< $@

clean:
	rm -f *.o *.elf *.bin
