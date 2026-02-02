#!/bin/sh

riscv64-unknown-elf-gcc -Wall -nostdlib -nostartfiles -g \
  -Wl,-Ttext=0x1000 \
  -Wl,--defsym=__stack_top=0x39F0 \
  -Wl,--defsym=_start=0x1000 \
  vmstartup.S $1

riscv64-unknown-elf-objcopy -O binary a.out a.bin
