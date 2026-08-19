    .section .text.init, "ax"
    .globl _start
    .type _start, @function
_start:
    la sp, __stack_top

    la t0, __bss_start
    la t1, __bss_end
1:
    bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:
    call main
3:
    wfi
    j 3b
    .size _start, . - _start

    .section .guest_elf, "a", @progbits
    .balign 4096
    .global __guest_elf
__guest_elf:
    .incbin GUEST_ELF
    .global __guest_elf_size
    .set __guest_elf_size, . - __guest_elf
