/*
 * This file contains functions used to handle the trap. Basically, this is a 1:1 port from
 * https://github.com/riscv-software-src/opensbi/blob/master/firmware/fw_base.S
 *
 * We need to expose the _trap_handler function which is executed when a trap occurs.
 * We are forwarding everything to `sbi_trap_handler`.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

use riscv::{
    interrupt::supervisor::Exception,
    register::mstatus::{self, MPP},
};

use crate::{
    opensbi,
    shadowfax_core::state::{HartSupervisorStateArea, TsmSupervisorStateArea, STATE},
};
use core::mem::offset_of;

macro_rules! cove_unpack_fid {
    ($fid:expr) => {
        (($fid >> 26) & 0x3F, $fid & 0xFFFF)
    };
}

/// The main trap handler function that orchestrates the saving and restoring of registers.
/// The handler verifies if the trap is a TEECALL/TEERESUME or a TEERET and handles is with custom
/// logic otherwise it applies opensbi logic.
#[repr(align(4))]
pub unsafe extern "C" fn handler() -> ! {
    /*
     * Check if the trap is a TEECALL/TEERESUME and perform the context switch to the tsm
     */
    core::arch::asm!(
        // Save t0 and t1 on the stack
        "addi sp, sp, -16",
        "sd t0, 0*8(sp)",
        "sd t1, 1*8(sp)",

        // Check if the trap is an ecall
        "csrr t0, mcause",
        "li t1, {ecall_code}",
        "bne t0, t1, 1f",

        // Check if the covh extension is invoked
        "add t0, a7, zero",
        "li t1, {covh_ext_id}",
        "bne t0, t1, 1f",

        // Restore t0 and t1 and jump to custom logic for TEECALL
        "ld t0, 0*8(sp)",
        "ld t1, 1*8(sp)",
        "addi sp, sp, 16",
        "j {tee_call_handler}",

        "1:",

        // Restore t0 and t1 before proceeding to opensbi flow
        "ld t0, 0*8(sp)",
        "ld t1, 1*8(sp)",
        "addi sp, sp, 16",

        ecall_code = const Exception::SupervisorEnvCall as usize,
        covh_ext_id = const 0x434F5648,
        tee_call_handler = sym tee_call_entry,
        options(preserves_flags),
    );
    /*
     * Saves the current stack pointer and sets up the stack pointer for the trap context.
     * It also swaps the TP and MSCRATCH registers.
     */
    core::arch::asm!(
        // Swap TP and MSCRATCH
        "csrrw tp, mscratch, tp",
        "sd t0, {sbi_scratch_tmp0_offset}(tp)",
        /*
         * From fw_base.S
         * Set T0 to appropriate exception stack
         *
         * Came_From_M_Mode = ((MSTATUS.MPP < PRV_M) ? 1 : 0) - 1;
         * Exception_Stack = TP ^ (Came_From_M_Mode & (SP ^ TP))
         *
         * Came_From_M_Mode = 0    ==>    Exception_Stack = TP
         * Came_From_M_Mode = -1   ==>    Exception_Stack = SP
         */
        "csrr t0, mstatus",
        "srl t0, t0, {mstatus_mpp_shift}",
        "and t0, t0, 3",
        "slti t0, t0, 3",
        "add t0, t0, -1",
        "xor sp, sp, tp",
        "and t0, t0, sp",
        "xor sp, sp, tp",
        "xor t0, tp, t0",
        // Save original SP on exception st
        "sd sp,  ({sbi_trap_regs_offset_sp}-{sbi_trap_context_size})(t0)",
        // Set SP to exception stack and make room for trap context
        "add sp, t0, -{sbi_trap_context_size}",
        // Restore T0 from scratch space
        "ld t0, {sbi_scratch_tmp0_offset}(tp)",
        // Save T0 on stack
        "sd t0, {sbi_trap_regs_offset_t0}(sp)",
        // Swap TP and MSCRATCH
        "csrrw tp, mscratch, tp",
        mstatus_mpp_shift = const opensbi::MSTATUS_MPP_SHIFT,
        sbi_trap_context_size = const (size_of::<opensbi::sbi_trap_regs>() + size_of::<opensbi::sbi_trap_info>() ) ,
        sbi_trap_regs_offset_sp = const offset_of!(opensbi::sbi_trap_regs, sp),
        sbi_trap_regs_offset_t0 = const offset_of!(opensbi::sbi_trap_regs, t0),
        sbi_scratch_tmp0_offset = const offset_of!(opensbi::sbi_scratch, tmp0),

    );
    /*
     * Saves the machine exception program counter (MEPC) and machine status (MSTATUS) registers
     * to the trap context stack.
     */
    core::arch::asm!(
        "csrr t0, mepc",
        "sd t0, {sbi_trap_regs_offset_mepc}(sp)",
        "csrr t0, mstatus",
        "sd t0, {sbi_trap_regs_offset_mstatus}(sp)",
        "sd zero, {sbi_trap_regs_offset_mstatush}(sp)",
        sbi_trap_regs_offset_mepc = const offset_of!(opensbi::sbi_trap_regs, mepc),
        sbi_trap_regs_offset_mstatus= const offset_of!(opensbi::sbi_trap_regs, mstatus),
        sbi_trap_regs_offset_mstatush = const offset_of!(opensbi::sbi_trap_regs, mstatusH),
    );
    /*
     * Saves additional trap information such as cause and trap value to the trap context stack.
     * Clears the machine-dependent trap (MDT) register.
     */
    core::arch::asm!(
        "sd zero, {sbi_trap_regs_offset_zero}(sp)",
        "sd ra, {sbi_trap_regs_offset_ra}(sp)",
        "sd gp, {sbi_trap_regs_offset_gp}(sp)",
        "sd tp, {sbi_trap_regs_offset_tp}(sp)",
        "sd t1, {sbi_trap_regs_offset_t1}(sp)",
        "sd t2, {sbi_trap_regs_offset_t2}(sp)",
        "sd s0, {sbi_trap_regs_offset_s0}(sp)",
        "sd s1, {sbi_trap_regs_offset_s1}(sp)",
        "sd a0, {sbi_trap_regs_offset_a0}(sp)",
        "sd a1, {sbi_trap_regs_offset_a1}(sp)",
        "sd a2, {sbi_trap_regs_offset_a2}(sp)",
        "sd a3, {sbi_trap_regs_offset_a3}(sp)",
        "sd a4, {sbi_trap_regs_offset_a4}(sp)",
        "sd a5, {sbi_trap_regs_offset_a5}(sp)",
        "sd a6, {sbi_trap_regs_offset_a6}(sp)",
        "sd a7, {sbi_trap_regs_offset_a7}(sp)",
        "sd s2, {sbi_trap_regs_offset_s2}(sp)",
        "sd s3, {sbi_trap_regs_offset_s3}(sp)",
        "sd s4, {sbi_trap_regs_offset_s4}(sp)",
        "sd s5, {sbi_trap_regs_offset_s5}(sp)",
        "sd s6, {sbi_trap_regs_offset_s6}(sp)",
        "sd s7, {sbi_trap_regs_offset_s7}(sp)",
        "sd s8, {sbi_trap_regs_offset_s8}(sp)",
        "sd s9, {sbi_trap_regs_offset_s9}(sp)",
        "sd s10, {sbi_trap_regs_offset_s10}(sp)",
        "sd s11, {sbi_trap_regs_offset_s11}(sp)",
        "sd t3, {sbi_trap_regs_offset_t3}(sp)",
        "sd t4, {sbi_trap_regs_offset_t4}(sp)",
        "sd t5, {sbi_trap_regs_offset_t5}(sp)",
        "sd t6, {sbi_trap_regs_offset_t6}(sp)",
        sbi_trap_regs_offset_zero = const offset_of!(opensbi::sbi_trap_regs, zero),
        sbi_trap_regs_offset_ra = const offset_of!(opensbi::sbi_trap_regs, ra),
        sbi_trap_regs_offset_gp = const offset_of!(opensbi::sbi_trap_regs, gp),
        sbi_trap_regs_offset_tp = const offset_of!(opensbi::sbi_trap_regs, tp),
        sbi_trap_regs_offset_t1 = const offset_of!(opensbi::sbi_trap_regs, t1),
        sbi_trap_regs_offset_t2 = const offset_of!(opensbi::sbi_trap_regs, t2),
        sbi_trap_regs_offset_s0 = const offset_of!(opensbi::sbi_trap_regs, s0),
        sbi_trap_regs_offset_s1 = const offset_of!(opensbi::sbi_trap_regs, s1),
        sbi_trap_regs_offset_a0 = const offset_of!(opensbi::sbi_trap_regs, a0),
        sbi_trap_regs_offset_a1 = const offset_of!(opensbi::sbi_trap_regs, a1),
        sbi_trap_regs_offset_a2 = const offset_of!(opensbi::sbi_trap_regs, a2),
        sbi_trap_regs_offset_a3 = const offset_of!(opensbi::sbi_trap_regs, a3),
        sbi_trap_regs_offset_a4 = const offset_of!(opensbi::sbi_trap_regs, a4),
        sbi_trap_regs_offset_a5 = const offset_of!(opensbi::sbi_trap_regs, a5),
        sbi_trap_regs_offset_a6 = const offset_of!(opensbi::sbi_trap_regs, a6),
        sbi_trap_regs_offset_a7 = const offset_of!(opensbi::sbi_trap_regs, a7),
        sbi_trap_regs_offset_s2 = const offset_of!(opensbi::sbi_trap_regs, s2),
        sbi_trap_regs_offset_s3 = const offset_of!(opensbi::sbi_trap_regs, s3),
        sbi_trap_regs_offset_s4 = const offset_of!(opensbi::sbi_trap_regs, s4),
        sbi_trap_regs_offset_s5 = const offset_of!(opensbi::sbi_trap_regs, s5),
        sbi_trap_regs_offset_s6 = const offset_of!(opensbi::sbi_trap_regs, s6),
        sbi_trap_regs_offset_s7 = const offset_of!(opensbi::sbi_trap_regs, s7),
        sbi_trap_regs_offset_s8 = const offset_of!(opensbi::sbi_trap_regs, s8),
        sbi_trap_regs_offset_s9 = const offset_of!(opensbi::sbi_trap_regs, s9),
        sbi_trap_regs_offset_s10 = const offset_of!(opensbi::sbi_trap_regs, s10),
        sbi_trap_regs_offset_s11 = const offset_of!(opensbi::sbi_trap_regs, s11),
        sbi_trap_regs_offset_t3 = const offset_of!(opensbi::sbi_trap_regs, t3),
        sbi_trap_regs_offset_t4 = const offset_of!(opensbi::sbi_trap_regs, t4),
        sbi_trap_regs_offset_t5 = const offset_of!(opensbi::sbi_trap_regs, t5),
        sbi_trap_regs_offset_t6 = const offset_of!(opensbi::sbi_trap_regs, t6),
    );
    core::arch::asm!(
        "csrr t0, mcause",
        "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_cause})(sp)",
        "csrr t0, mtval",
        "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tval})(sp)",
        "sd zero, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tval2})(sp)",
        "sd zero, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tinst})(sp)",
        "li t0, 0",
        "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_gva})(sp)",
        sbi_trap_regs_size = const size_of::<opensbi::sbi_trap_regs>(),
        sbi_trap_info_offset_cause = const  offset_of!(opensbi::sbi_trap_info, cause),
        sbi_trap_info_offset_tval = const offset_of!(opensbi::sbi_trap_info, tval),
        sbi_trap_info_offset_tval2 = const offset_of!(opensbi::sbi_trap_info, tval2),
        sbi_trap_info_offset_tinst = const offset_of!(opensbi::sbi_trap_info, tinst),
        sbi_trap_info_offset_gva = const offset_of!(opensbi::sbi_trap_info, gva),
    );
    // We can take another trap
    core::arch::asm!("li t0, 0x40000000000", "csrc mstatus, t0", options(nostack));

    /*
     * Call out trap handler which wraps opensbi trap handler
     */
    core::arch::asm!("add a0, sp, zero", "call {trap_handler}", trap_handler = sym opensbi::sbi_trap_handler);

    /*
     * Restores all general-purpose registers except A0 and T0 from the trap context stack.
     */
    core::arch::asm!(
        "ld ra, {sbi_trap_regs_offset_ra}(a0)",
        "ld sp, {sbi_trap_regs_offset_sp}(a0)",
        "ld gp, {sbi_trap_regs_offset_gp}(a0)",
        "ld tp, {sbi_trap_regs_offset_tp}(a0)",
        "ld t1, {sbi_trap_regs_offset_t1}(a0)",
        "ld t2, {sbi_trap_regs_offset_t2}(a0)",
        "ld s0, {sbi_trap_regs_offset_s0}(a0)",
        "ld s1, {sbi_trap_regs_offset_s1}(a0)",
        "ld a1, {sbi_trap_regs_offset_a1}(a0)",
        "ld a2, {sbi_trap_regs_offset_a2}(a0)",
        "ld a3, {sbi_trap_regs_offset_a3}(a0)",
        "ld a4, {sbi_trap_regs_offset_a4}(a0)",
        "ld a5, {sbi_trap_regs_offset_a5}(a0)",
        "ld a6, {sbi_trap_regs_offset_a6}(a0)",
        "ld a7, {sbi_trap_regs_offset_a7}(a0)",
        "ld s2, {sbi_trap_regs_offset_s2}(a0)",
        "ld s3, {sbi_trap_regs_offset_s3}(a0)",
        "ld s4, {sbi_trap_regs_offset_s4}(a0)",
        "ld s5, {sbi_trap_regs_offset_s5}(a0)",
        "ld s6, {sbi_trap_regs_offset_s6}(a0)",
        "ld s7, {sbi_trap_regs_offset_s7}(a0)",
        "ld s8, {sbi_trap_regs_offset_s8}(a0)",
        "ld s9, {sbi_trap_regs_offset_s9}(a0)",
        "ld s10, {sbi_trap_regs_offset_s10}(a0)",
        "ld s11, {sbi_trap_regs_offset_s11}(a0)",
        "ld t3, {sbi_trap_regs_offset_t3}(a0)",
        "ld t4, {sbi_trap_regs_offset_t4}(a0)",
        "ld t5, {sbi_trap_regs_offset_t5}(a0)",
        "ld t6, {sbi_trap_regs_offset_t6}(a0)",
        sbi_trap_regs_offset_sp = const offset_of!(opensbi::sbi_trap_regs, sp),
        sbi_trap_regs_offset_ra = const offset_of!(opensbi::sbi_trap_regs, ra),
        sbi_trap_regs_offset_gp = const offset_of!(opensbi::sbi_trap_regs, gp),
        sbi_trap_regs_offset_tp = const offset_of!(opensbi::sbi_trap_regs, tp),
        sbi_trap_regs_offset_t1 = const offset_of!(opensbi::sbi_trap_regs, t1),
        sbi_trap_regs_offset_t2 = const offset_of!(opensbi::sbi_trap_regs, t2),
        sbi_trap_regs_offset_s0 = const offset_of!(opensbi::sbi_trap_regs, s0),
        sbi_trap_regs_offset_s1 = const offset_of!(opensbi::sbi_trap_regs, s1),
        sbi_trap_regs_offset_a1 = const offset_of!(opensbi::sbi_trap_regs, a1),
        sbi_trap_regs_offset_a2 = const offset_of!(opensbi::sbi_trap_regs, a2),
        sbi_trap_regs_offset_a3 = const offset_of!(opensbi::sbi_trap_regs, a3),
        sbi_trap_regs_offset_a4 = const offset_of!(opensbi::sbi_trap_regs, a4),
        sbi_trap_regs_offset_a5 = const offset_of!(opensbi::sbi_trap_regs, a5),
        sbi_trap_regs_offset_a6 = const offset_of!(opensbi::sbi_trap_regs, a6),
        sbi_trap_regs_offset_a7 = const offset_of!(opensbi::sbi_trap_regs, a7),
        sbi_trap_regs_offset_s2 = const offset_of!(opensbi::sbi_trap_regs, s2),
        sbi_trap_regs_offset_s3 = const offset_of!(opensbi::sbi_trap_regs, s3),
        sbi_trap_regs_offset_s4 = const offset_of!(opensbi::sbi_trap_regs, s4),
        sbi_trap_regs_offset_s5 = const offset_of!(opensbi::sbi_trap_regs, s5),
        sbi_trap_regs_offset_s6 = const offset_of!(opensbi::sbi_trap_regs, s6),
        sbi_trap_regs_offset_s7 = const offset_of!(opensbi::sbi_trap_regs, s7),
        sbi_trap_regs_offset_s8 = const offset_of!(opensbi::sbi_trap_regs, s8),
        sbi_trap_regs_offset_s9 = const offset_of!(opensbi::sbi_trap_regs, s9),
        sbi_trap_regs_offset_s10 = const offset_of!(opensbi::sbi_trap_regs, s10),
        sbi_trap_regs_offset_s11 = const offset_of!(opensbi::sbi_trap_regs, s11),
        sbi_trap_regs_offset_t3 = const offset_of!(opensbi::sbi_trap_regs, t3),
        sbi_trap_regs_offset_t4 = const offset_of!(opensbi::sbi_trap_regs, t4),
        sbi_trap_regs_offset_t5 = const offset_of!(opensbi::sbi_trap_regs, t5),
        sbi_trap_regs_offset_t6 = const offset_of!(opensbi::sbi_trap_regs, t6),
    );
    /*
     * Restores the machine status (MSTATUS) and machine exception program counter (MEPC)
     * registers from the trap context stack.
     */
    core::arch::asm!(
        "ld t0, {sbi_trap_regs_offset_mstatus}(a0)",
        "csrw mstatus, t0",
        "ld t0, {sbi_trap_regs_offset_mepc}(a0)",
        "csrw mepc, t0",
        sbi_trap_regs_offset_mstatus= const offset_of!(opensbi::sbi_trap_regs, mstatus),
        sbi_trap_regs_offset_mepc = const offset_of!(opensbi::sbi_trap_regs, mepc),
    );
    /*
     * Restores the A0 and T0 registers from the trap context stack.
     */
    core::arch::asm!(
        "ld t0, {sbi_trap_regs_offset_t0}(a0)",
        "ld a0, {sbi_trap_regs_offset_a0}(a0)",
        sbi_trap_regs_offset_t0 = const offset_of!(opensbi::sbi_trap_regs, t0),
        sbi_trap_regs_offset_a0 = const offset_of!(opensbi::sbi_trap_regs, a0),
    );

    core::arch::asm!("mret", options(noreturn))
}

fn tee_call_entry() -> ! {
    unsafe {
        core::arch::asm!(
            "j {handler}",
            handler = sym tee_call_handler,
            options(noreturn)
        )
    }
}

/// The first part of this function is used to search the target supervisor domain.
/// Before entering, tee_call_entry has saved some registers to give us the ability
/// to use high-level code.
/// Right before the context switch, we need to restore these registers.
unsafe extern "C" fn tee_call_handler() -> ! {
    // 1) retrieve the supervisor domain and mark it as active
    let mut fid: usize;
    unsafe {
        core::arch::asm!("add {}, a6, zero", out(reg) fid, options(readonly, nostack));
    }
    let (sdid, _fid) = cove_unpack_fid!(fid);
    let mut state_guard = STATE.lock();
    let state = state_guard.get_mut().unwrap();
    let domain = state.domains.get_mut(sdid).unwrap();

    domain.active = true;

    let tsm = domain.tsm.as_ref().unwrap();
    let sp = tsm.stack_pointer;

    // release the lock
    drop(state_guard);

    riscv::asm::fence_i();

    // 2) save hssa context
    unsafe {
        core::arch::asm!(
            // jump to the dedicated stack
            "mv sp, {stack_top}",
            // make room for the context and save all registers
            "addi sp, sp, -{hssa_size}",

            // save gprs registers
            "
            .align 4
            sd zero, 0(sp)
            sd ra, 1*8(sp)
            sd gp, 3*8(sp)
            sd tp, 4*8(sp)
            sd t0, 5*8(sp)
            sd t1, 6*8(sp)
            sd t2, 7*8(sp)
            sd s0, 8*8(sp)
            sd s1, 9*8(sp)
            sd a2, 12*8(sp)
            sd a3, 13*8(sp)
            sd a4, 14*8(sp)
            sd a5, 15*8(sp)
            sd a6, 16*8(sp)
            sd a7, 17*8(sp)
            sd s2, 18*8(sp)
            sd s3, 19*8(sp)
            sd s4, 20*8(sp)
            sd s5, 21*8(sp)
            sd s6, 22*8(sp)
            sd s7, 23*8(sp)
            sd s8, 24*8(sp)
            sd s9, 25*8(sp)
            sd s10, 26*8(sp)
            sd s11, 27*8(sp)
            sd t3, 28*8(sp)
            sd t4, 29*8(sp)
            sd t5, 30*8(sp)
            sd t6, 31*8(sp)",

            // save CSRs
            "
            csrr t0, sstatus
            sd t0, 32*8(sp)
            csrr t0, stvec
            sd t0, 33*8(sp)
            csrr t0, sip
            sd t0, 34*8(sp)
            csrr t0, scounteren
            sd t0, 35*8(sp)
            csrr t0, sscratch
            sd t0, 36*8(sp)
            csrr t0, satp
            sd t0, 37*8(sp)
            csrr t0, senvcfg
            sd t0, 38*8(sp)
            // csrr t0, scontext
            // sd t0, 39*8(sp)
            csrr t0, mepc
            sd t0, 40*8(sp)",
            stack_top = in(reg) sp,
            hssa_size = const core::mem::size_of::<HartSupervisorStateArea>()
        )
    };

    let mut mstatus = mstatus::read();
    mstatus.set_mpp(MPP::Supervisor);
    mstatus.set_sie(false);

    // re-program pmp
    let mut scratch: usize;
    unsafe { core::arch::asm!("add {}, tp, zero", out(reg) scratch, options(nostack, nomem)) }
    let n = opensbi::sbi_hart_pmp_count(scratch as *mut opensbi::sbi_scratch);

    for i in 0..n {
        opensbi::pmp_disable(i);
    }

    // 3) restore tssa context
    unsafe {
        core::arch::asm!(
            // jump to the dedicated stack
            "mv sp, {stack_top}",
            // jump to the beginning of the context
            "addi sp, sp, -({hssa_size} + {tssa_size})",

            // restore gprs registers
            "
            .align 4
            ld zero, 0(sp)
            ld ra, 1*8(sp)
            ld gp, 3*8(sp)
            ld tp, 4*8(sp)
            ld t0, 5*8(sp)
            ld t1, 6*8(sp)
            ld t2, 7*8(sp)
            ld s0, 8*8(sp)
            ld s1, 9*8(sp)
            ld a2, 12*8(sp)
            ld a3, 13*8(sp)
            ld a4, 14*8(sp)
            ld a5, 15*8(sp)
            ld a6, 16*8(sp)
            ld a7, 17*8(sp)
            ld s2, 18*8(sp)
            ld s3, 19*8(sp)
            ld s4, 20*8(sp)
            ld s5, 21*8(sp)
            ld s6, 22*8(sp)
            ld s7, 23*8(sp)
            ld s8, 24*8(sp)
            ld s9, 25*8(sp)
            ld s10, 26*8(sp)
            ld s11, 27*8(sp)
            ld t3, 28*8(sp)
            ld t4, 29*8(sp)
            ld t5, 30*8(sp)
            ld t6, 31*8(sp)
            ",

            // save CSRs
            "
            ld t0, 32*8(sp)
            csrw sstatus, t0
            ld t0, 33*8(sp)
            csrw stvec, t0
            ld t0, 34*8(sp)
            csrw sip, t0
            ld t0, 35*8(sp)
            csrw scounteren, t0
            ld t0, 36*8(sp)
            csrw sscratch, t0
            ld t0, 37*8(sp)
            csrw satp, t0
            ld t0, 38*8(sp)
            csrw senvcfg, t0
            // ld t0, 39*8(sp)
            // csrw scontext, t0
            ld t0, 40*8(sp)
            csrw mepc, t0
            ",
            stack_top = in(reg) sp,
            hssa_size = const core::mem::size_of::<HartSupervisorStateArea>(),
            tssa_size = const core::mem::size_of::<TsmSupervisorStateArea>()
        )
    };

    unsafe { core::arch::asm!("mret", options(noreturn)) }
}
