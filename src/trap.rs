/*
 * This file contains functions used to handle the trap. Basically, this is a 1:1 port from
 * https://github.com/riscv-software-src/opensbi/blob/master/firmware/fw_base.S
 *
 * We need to expose the _trap_handler function which is executed when a trap occurs.
 * We are forwarding everything to `sbi_trap_handler`.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

use riscv::interrupt::supervisor::Exception;

use crate::{cove, opensbi};
use core::mem::offset_of;

/// The main trap handler function that orchestrates the saving and restoring of registers.
/// The handler verifies if the trap is a TEECALL/TEERESUME or a TEERET and handles it with custom
/// logic.
#[rustc_align(4)]
#[unsafe(naked)]
pub fn handler() -> ! {
    core::arch::naked_asm!(
        /*
         * Check if the trap is a TEECALL/TEERET and perform the context switch to the tsm
         * TODO: a7 may be tampered
         */
        "
            csrrw tp, mscratch, tp
            sd t0, {sbi_scratch_tmp0_offset}(tp)
            csrr t0, mcause
            add t0, t0, -{ecall_code}
            bnez t0, 1f
            li t0, {covh_ext_id}
            sub t0, a7, t0
            beqz t0, {tee_handler}
            li t0, {supd_ext_id}
            sub t0, a7, t0
            beqz t0, {supd_handler}
            1:
        ",
        /*
         * Saves the current stack pointer and sets up the stack pointer for the trap context.
         * It also swaps the TP and MSCRATCH registers.
         *
         * From fw_base.S
         * Set T0 to appropriate exception stack
         *
         * Came_From_M_Mode = ((MSTATUS.MPP < PRV_M) ? 1 : 0) - 1;
         * Exception_Stack = TP ^ (Came_From_M_Mode & (SP ^ TP))
         *
         * Came_From_M_Mode = 0    ==>    Exception_Stack = TP
         * Came_From_M_Mode = -1   ==>    Exception_Stack = SP
         */
        "
            csrr t0, mstatus
            srl t0, t0, {mstatus_mpp_shift}
            and t0, t0, 3
            slti t0, t0, 3
            add t0, t0, -1
            xor sp, sp, tp
            and t0, t0, sp
            xor sp, sp, tp
            xor t0, tp, t0

            // Save original SP on exception st
            sd sp,  ({sbi_trap_regs_offset_sp}-{sbi_trap_context_size})(t0)

            // Set SP to exception stack and make room for trap context
            add sp, t0, -{sbi_trap_context_size}

            // Restore T0 from scratch space
            ld t0, {sbi_scratch_tmp0_offset}(tp)

            // Save T0 on stack
            sd t0, {sbi_trap_regs_offset_t0}(sp)

            // Swap TP and MSCRATCH
            csrrw tp, mscratch, tp
        ",

        /*
         * Saves the machine exception program counter (MEPC) and machine status (MSTATUS) registers
         * to the trap context stack.
         */
        "
            csrr t0, mepc
            sd t0, {sbi_trap_regs_offset_mepc}(sp)
            csrr t0, mstatus
            sd t0, {sbi_trap_regs_offset_mstatus}(sp)
            sd zero, {sbi_trap_regs_offset_mstatush}(sp)
        ",

        /*
         * Saves additional trap information such as cause and trap value to the trap context stack.
         * Clears the machine-dependent trap (MDT) register.
         */
        "
            sd zero, {sbi_trap_regs_offset_zero}(sp)
            sd ra, {sbi_trap_regs_offset_ra}(sp)
            sd gp, {sbi_trap_regs_offset_gp}(sp)
            sd tp, {sbi_trap_regs_offset_tp}(sp)
            sd t1, {sbi_trap_regs_offset_t1}(sp)
            sd t2, {sbi_trap_regs_offset_t2}(sp)
            sd s0, {sbi_trap_regs_offset_s0}(sp)
            sd s1, {sbi_trap_regs_offset_s1}(sp)
            sd a0, {sbi_trap_regs_offset_a0}(sp)
            sd a1, {sbi_trap_regs_offset_a1}(sp)
            sd a2, {sbi_trap_regs_offset_a2}(sp)
            sd a3, {sbi_trap_regs_offset_a3}(sp)
            sd a4, {sbi_trap_regs_offset_a4}(sp)
            sd a5, {sbi_trap_regs_offset_a5}(sp)
            sd a6, {sbi_trap_regs_offset_a6}(sp)
            sd a7, {sbi_trap_regs_offset_a7}(sp)
            sd s2, {sbi_trap_regs_offset_s2}(sp)
            sd s3, {sbi_trap_regs_offset_s3}(sp)
            sd s4, {sbi_trap_regs_offset_s4}(sp)
            sd s5, {sbi_trap_regs_offset_s5}(sp)
            sd s6, {sbi_trap_regs_offset_s6}(sp)
            sd s7, {sbi_trap_regs_offset_s7}(sp)
            sd s8, {sbi_trap_regs_offset_s8}(sp)
            sd s9, {sbi_trap_regs_offset_s9}(sp)
            sd s10, {sbi_trap_regs_offset_s10}(sp)
            sd s11, {sbi_trap_regs_offset_s11}(sp)
            sd t3, {sbi_trap_regs_offset_t3}(sp)
            sd t4, {sbi_trap_regs_offset_t4}(sp)
            sd t5, {sbi_trap_regs_offset_t5}(sp)
            sd t6, {sbi_trap_regs_offset_t6}(sp)
        ",

        "
            csrr t0, mcause
            sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_cause})(sp)
            csrr t0, mtval
            sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tval})(sp)
            sd zero, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tval2})(sp)
            sd zero, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tinst})(sp)
            li t0, 0
            sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_gva})(sp)
        ",

        // We can take another trap
        "
            li t0, 0x40000000000
            csrc mstatus, t0
        ",

        /*
         * Call the opensbi trap handler which wraps opensbi trap handler
         */
        "
            add a0, sp, zero
            call {trap_handler}
        ",

        /*
         * Restores all general-purpose registers except A0 and T0 from the trap context stack.
         */
        "
            ld ra, {sbi_trap_regs_offset_ra}(a0)
            ld sp, {sbi_trap_regs_offset_sp}(a0)
            ld gp, {sbi_trap_regs_offset_gp}(a0)
            ld tp, {sbi_trap_regs_offset_tp}(a0)
            ld t1, {sbi_trap_regs_offset_t1}(a0)
            ld t2, {sbi_trap_regs_offset_t2}(a0)
            ld s0, {sbi_trap_regs_offset_s0}(a0)
            ld s1, {sbi_trap_regs_offset_s1}(a0)
            ld a1, {sbi_trap_regs_offset_a1}(a0)
            ld a2, {sbi_trap_regs_offset_a2}(a0)
            ld a3, {sbi_trap_regs_offset_a3}(a0)
            ld a4, {sbi_trap_regs_offset_a4}(a0)
            ld a5, {sbi_trap_regs_offset_a5}(a0)
            ld a6, {sbi_trap_regs_offset_a6}(a0)
            ld a7, {sbi_trap_regs_offset_a7}(a0)
            ld s2, {sbi_trap_regs_offset_s2}(a0)
            ld s3, {sbi_trap_regs_offset_s3}(a0)
            ld s4, {sbi_trap_regs_offset_s4}(a0)
            ld s5, {sbi_trap_regs_offset_s5}(a0)
            ld s6, {sbi_trap_regs_offset_s6}(a0)
            ld s7, {sbi_trap_regs_offset_s7}(a0)
            ld s8, {sbi_trap_regs_offset_s8}(a0)
            ld s9, {sbi_trap_regs_offset_s9}(a0)
            ld s10, {sbi_trap_regs_offset_s10}(a0)
            ld s11, {sbi_trap_regs_offset_s11}(a0)
            ld t3, {sbi_trap_regs_offset_t3}(a0)
            ld t4, {sbi_trap_regs_offset_t4}(a0)
            ld t5, {sbi_trap_regs_offset_t5}(a0)
            ld t6, {sbi_trap_regs_offset_t6}(a0)
        ",

        /*
         * Restores the machine status (MSTATUS) and machine exception program counter (MEPC)
         * registers from the trap context stack.
         */
        "
            ld t0, {sbi_trap_regs_offset_mstatus}(a0)
            csrw mstatus, t0
            ld t0, {sbi_trap_regs_offset_mepc}(a0)
            csrw mepc, t0
        ",

        /*
         * Restores the A0 and T0 registers from the trap context stack.
         */

        "
            ld t0, {sbi_trap_regs_offset_t0}(a0)
            ld a0, {sbi_trap_regs_offset_a0}(a0)
        ",

        /*
         * Go back to caller
         */
        "mret",

        sbi_scratch_tmp0_offset = const offset_of!(opensbi::sbi_scratch, tmp0),
        ecall_code = const Exception::SupervisorEnvCall as usize,
        covh_ext_id = const cove::COVH_EXT_ID,
        supd_ext_id = const cove::SUPD_EXT_ID,
        tee_handler = sym cove::tee_handler_entry,
        supd_handler = sym cove::supd_handler_entry,

        mstatus_mpp_shift = const opensbi::MSTATUS_MPP_SHIFT,
        sbi_trap_context_size = const (size_of::<opensbi::sbi_trap_regs>() + size_of::<opensbi::sbi_trap_info>() ) ,
        sbi_trap_regs_offset_sp = const offset_of!(opensbi::sbi_trap_regs, sp),
        sbi_trap_regs_offset_t0 = const offset_of!(opensbi::sbi_trap_regs, t0),

        sbi_trap_regs_offset_mepc = const offset_of!(opensbi::sbi_trap_regs, mepc),
        sbi_trap_regs_offset_mstatus= const offset_of!(opensbi::sbi_trap_regs, mstatus),
        sbi_trap_regs_offset_mstatush = const offset_of!(opensbi::sbi_trap_regs, mstatusH),

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

        sbi_trap_regs_size = const size_of::<opensbi::sbi_trap_regs>(),
        sbi_trap_info_offset_cause = const  offset_of!(opensbi::sbi_trap_info, cause),
        sbi_trap_info_offset_tval = const offset_of!(opensbi::sbi_trap_info, tval),
        sbi_trap_info_offset_tval2 = const offset_of!(opensbi::sbi_trap_info, tval2),
        sbi_trap_info_offset_tinst = const offset_of!(opensbi::sbi_trap_info, tinst),
        sbi_trap_info_offset_gva = const offset_of!(opensbi::sbi_trap_info, gva),

        trap_handler = sym opensbi::sbi_trap_handler
    );
}
