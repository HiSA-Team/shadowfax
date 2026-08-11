    .section .text.start, "ax"
    .globl _start
    .type _start, @function
_start:
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop

    la sp, __stack_top

    la t0, __bss_start
    la t1, __bss_end
1:
    bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b

2:
    li t0, 0x6000
#ifdef EMBENCH_SMODE
    csrs sstatus, t0
#else
    csrs mstatus, t0
#endif

    li a0, 0
    li a1, 0
    call main
    call embench_finish

3:
    j 3b
    .size _start, . - _start
