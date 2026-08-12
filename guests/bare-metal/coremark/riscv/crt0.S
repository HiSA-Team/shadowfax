    .section .text._start
    .globl _start
_start:
    /* clear BSS */
    la   t0, __bss_start
    la   t1, __bss_end
_loop:
    bgeu t0, t1, _resetregs
    sd   zero, 0(t0)
    addi t0, t0, 8
    j    _loop
_resetregs:
    /* Reset registers */
    mv ra, zero
    mv gp, zero
    mv tp, zero
    mv t0, zero
    mv t1, zero
    mv t2, zero
    mv s0, zero
    mv s1, zero
    mv a0, zero
    mv a1, zero
    mv a2, zero
    mv a3, zero
    mv a4, zero
    mv a5, zero
    mv a6, zero
    mv a7, zero
    mv s2, zero
    mv s3, zero
    mv s4, zero
    mv s5, zero
    mv s6, zero
    mv s7, zero
    mv s8, zero
    mv s9, zero
    mv s10, zero
    mv s11, zero
    mv t3, zero
    mv t4, zero
    mv t5, zero
    mv t6, zero

    /* Load stack pointer */
    la sp, __stack_top

    /* Zero out a0 and a1 register*/
    add a0, zero, zero
    add a1, zero, zero

    /* Call main */
    call main

    /* Hang */
__hang:
    j __hang
