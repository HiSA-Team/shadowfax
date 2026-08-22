    .section .text, "ax"
    .align 2

    .globl attack_load
attack_load:
    .option push
    .option norvc
    ld a0, 0(a0)
    ret
    .option pop

    .globl attacker_trap
attacker_trap:
    addi sp, sp, -32
    sd t0, 0(sp)
    sd t1, 8(sp)
    sd t2, 16(sp)
    csrr t0, scause
    li t1, 5
    bne t0, t1, 1f
    la t0, attack_faulted
    li t1, 1
    sd t1, 0(t0)
    csrr t0, sepc
    addi t0, t0, 4
    csrw sepc, t0
    j 2f
1:
    la t0, attack_faulted
    li t1, 2
    sd t1, 0(t0)
    j 1b
2:
    ld t0, 0(sp)
    ld t1, 8(sp)
    ld t2, 16(sp)
    addi sp, sp, 32
    sret
