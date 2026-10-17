//! Early boot: enter the trusted TSM in S-mode and return to the firmware.
//! Interrupts are already disabled, paging is off, and OpenSBI has not started.

use common::{sbi::SbiRet, tsm_abi::*};

/// Save the M-mode caller on its own stack. The TSM sets up its own stack;
/// mscratch keeps our stack pointer until INIT_DONE returns control here.
#[unsafe(naked)]
pub unsafe extern "C" fn enter(entry: usize, boot_info: usize, sscratch_out: *mut usize) -> SbiRet {
    core::arch::naked_asm!(
        "
        addi sp, sp, -176
        sd ra,   0(sp)
        sd gp,   8(sp)
        sd tp,  16(sp)
        sd s0,  24(sp)
        sd s1,  32(sp)
        sd s2,  40(sp)
        sd s3,  48(sp)
        sd s4,  56(sp)
        sd s5,  64(sp)
        sd s6,  72(sp)
        sd s7,  80(sp)
        sd s8,  88(sp)
        sd s9,  96(sp)
        sd s10, 104(sp)
        sd s11, 112(sp)

        csrr t0, mtvec
        sd t0, 120(sp)
        csrr t0, mscratch
        sd t0, 128(sp)
        csrr t0, mstatus
        sd t0, 136(sp)
        csrr t0, medeleg
        sd t0, 144(sp)
        csrr t0, mepc
        sd t0, 152(sp)
        sd a2, 168(sp)

        csrw mscratch, sp
        lla t0, {handler}
        csrw mtvec, t0
        csrw medeleg, zero

        // Select S-mode for mret. Boot is non-virtualized with interrupts off.
        li t0, {mpp_mask}
        csrc mstatus, t0
        li t0, {mpp_supervisor}
        csrs mstatus, t0
        csrw mepc, a0
        mv a0, a1
        li a6, {function}
        li a7, {extension}
        fence.i
        mret
        ",
        mpp_mask = const 3 << 11,
        mpp_supervisor = const 1 << 11,
        extension = const SBI_SHADOWFAX_BOOT_EXT_ID,
        function = const SBI_SHADOWFAX_INIT_DONE,
        handler = sym init_done,
    );
}

#[rustc_align(4)]
#[unsafe(naked)]
unsafe extern "C" fn init_done() -> ! {
    core::arch::naked_asm!(
        "
        // This trap is already in M-mode. Recover the suspended firmware stack.
        csrr sp, mscratch
        csrr t0, sscratch
        sd t0, 160(sp)
        ld t1, 168(sp)
        sd t0, 0(t1)
        csrr t0, mcause
        li t1, 9
        bne t0, t1, 2f
        li t0, {extension}
        bne a7, t0, 2f
        li t0, {function}
        beq a6, t0, 3f
        2:
        li a0, -1
        li a1, {unexpected}
        3:
        ld t0, 120(sp)
        csrw mtvec, t0
        ld t0, 128(sp)
        csrw mscratch, t0
        ld t0, 136(sp)
        csrw mstatus, t0
        ld t0, 144(sp)
        csrw medeleg, t0
        ld t0, 152(sp)
        csrw mepc, t0

        ld ra,   0(sp)
        ld gp,   8(sp)
        ld tp,  16(sp)
        ld s0,  24(sp)
        ld s1,  32(sp)
        ld s2,  40(sp)
        ld s3,  48(sp)
        ld s4,  56(sp)
        ld s5,  64(sp)
        ld s6,  72(sp)
        ld s7,  80(sp)
        ld s8,  88(sp)
        ld s9,  96(sp)
        ld s10, 104(sp)
        ld s11, 112(sp)
        addi sp, sp, 176
        // a0/a1 are the TSM's result. Return to boot_tsm(), not to the TSM.
        ret
        ",
        extension = const SBI_SHADOWFAX_BOOT_EXT_ID,
        function = const SBI_SHADOWFAX_INIT_DONE,
        unexpected = const BOOT_UNEXPECTED_TRAP,
    );
}
