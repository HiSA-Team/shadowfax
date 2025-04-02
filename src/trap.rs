use crate::opensbi;
use core::{arch::asm, mem};

#[no_mangle]
#[link_section = ".entry"]
pub fn _trap_handler() {
    trap_save_and_setup_sp_t0();
    trap_save_mepc_status();
    trap_save_general_regs_except_sp_t0();
    trap_save_info();
    trap_call_c_routine();
    trap_restore_general_regs_except_a0_t0();
    trap_restore_mepc_status();
    trap_restore_a0_t0()
}

#[no_mangle]
#[inline(always)]
fn trap_save_and_setup_sp_t0() {
    unsafe {
        asm!(
            //  Swap TP and MSCRATCH
            "csrrw tp, {csr_mscratch}, tp",
            "sd t0, {sbi_scratch_tmp0_offset}(tp)",
            /*
             * Set T0 to appropriate exception stack
             *
             * Came_From_M_Mode = ((MSTATUS.MPP < PRV_M) ? 1 : 0) - 1;
             * Exception_Stack = TP ^ (Came_From_M_Mode & (SP ^ TP))
             *
             * Came_From_M_Mode = 0    ==>    Exception_Stack = TP
             * Came_From_M_Mode = -1   ==>    Exception_Stack = SP
             */
            "csrr t0, {csr_mstatus}",
            "srl t0, t0, {mstatus_mpp_shift}",
            "and t0, t0, {priv_m}",
            "slti t0, t0, {priv_m}",
            "add t0, t0, -1",
            "xor sp, sp, tp",
            "and t0, t0, sp",
            "xor sp, sp, tp",
            "xor t0, tp, t0",
            // Save original SP on exception st
            "sd sp,  ({sbi_trap_regs_offset_tp}-{sbi_trap_context_size})(t0)",
            // Set SP to exception stack and make room for trap context
            "add sp, t0, -{sbi_trap_context_size}",
            // Restore T0 from scratch space
            "ld t0, {sbi_scratch_tmp0_offset}(tp)",
            // Save T0 on stack
            "sd t0, {sbi_trap_regs_offset_t0}(sp)",
            // Swap TP and MSCRATCH
            "csrrw tp, {csr_mscratch}, tp",

            csr_mscratch = const opensbi::CSR_MSCRATCH,
            csr_mstatus = const opensbi::CSR_MSTATUS,
            mstatus_mpp_shift = const opensbi::MSTATUS_MPP_SHIFT,
            sbi_trap_context_size = const (size_of::<opensbi::sbi_trap_regs>() + size_of::<opensbi::sbi_trap_info>() + 8),
            sbi_trap_regs_offset_tp = const opensbi::SBI_TRAP_REGS_tp * 8,
            sbi_trap_regs_offset_t0 = const opensbi::SBI_TRAP_REGS_t0 * 8,
            sbi_scratch_tmp0_offset = const mem::offset_of!(opensbi::sbi_scratch, tmp0),
            priv_m = const 3,

        )
    }
}
#[no_mangle]
#[inline(always)]
fn trap_save_mepc_status() {
    unsafe {
        asm!(
            "csrr t0, {csr_mepc}",
            "sd t0, {sbi_trap_regs_offset_mepc}(sp)",
            "csrr t0, {csr_mstatus}",
            "sd t0, {sbi_trap_regs_offset_mstatus}(sp)",
            "sd zero, {sbi_trap_regs_offset_mstatush}(sp)",
            csr_mstatus = const opensbi::CSR_MSTATUS,
            csr_mepc= const opensbi::CSR_MEPC,
            sbi_trap_regs_offset_mepc = const opensbi::SBI_TRAP_REGS_mepc * 8,
            sbi_trap_regs_offset_mstatus= const opensbi::SBI_TRAP_REGS_mstatus * 8,
            sbi_trap_regs_offset_mstatush = const opensbi::SBI_TRAP_REGS_mstatusH * 8,
        )
    }
}

#[no_mangle]
#[inline(always)]
fn trap_save_general_regs_except_sp_t0() {
    unsafe {
        asm!(
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
            sbi_trap_regs_offset_zero = const opensbi::SBI_TRAP_REGS_zero * 8,
            sbi_trap_regs_offset_ra = const opensbi::SBI_TRAP_REGS_ra * 8,
            sbi_trap_regs_offset_gp = const opensbi::SBI_TRAP_REGS_gp * 8,
            sbi_trap_regs_offset_tp = const opensbi::SBI_TRAP_REGS_tp * 8,
            sbi_trap_regs_offset_t1 = const opensbi::SBI_TRAP_REGS_t1 * 8,
            sbi_trap_regs_offset_t2 = const opensbi::SBI_TRAP_REGS_t2 * 8,
            sbi_trap_regs_offset_s0 = const opensbi::SBI_TRAP_REGS_s0 * 8,
            sbi_trap_regs_offset_s1 = const opensbi::SBI_TRAP_REGS_s1 * 8,
            sbi_trap_regs_offset_a0 = const opensbi::SBI_TRAP_REGS_a0 * 8,
            sbi_trap_regs_offset_a1 = const opensbi::SBI_TRAP_REGS_a1 * 8,
            sbi_trap_regs_offset_a2 = const opensbi::SBI_TRAP_REGS_a2 * 8,
            sbi_trap_regs_offset_a3 = const opensbi::SBI_TRAP_REGS_a3 * 8,
            sbi_trap_regs_offset_a4 = const opensbi::SBI_TRAP_REGS_a4 * 8,
            sbi_trap_regs_offset_a5 = const opensbi::SBI_TRAP_REGS_a5 * 8,
            sbi_trap_regs_offset_a6 = const opensbi::SBI_TRAP_REGS_a6 * 8,
            sbi_trap_regs_offset_a7 = const opensbi::SBI_TRAP_REGS_a7 * 8,
            sbi_trap_regs_offset_s2 = const opensbi::SBI_TRAP_REGS_s2 * 8,
            sbi_trap_regs_offset_s3 = const opensbi::SBI_TRAP_REGS_s3 * 8,
            sbi_trap_regs_offset_s4 = const opensbi::SBI_TRAP_REGS_s4 * 8,
            sbi_trap_regs_offset_s5 = const opensbi::SBI_TRAP_REGS_s5 * 8,
            sbi_trap_regs_offset_s6 = const opensbi::SBI_TRAP_REGS_s6 * 8,
            sbi_trap_regs_offset_s7 = const opensbi::SBI_TRAP_REGS_s7 * 8,
            sbi_trap_regs_offset_s8 = const opensbi::SBI_TRAP_REGS_s8 * 8,
            sbi_trap_regs_offset_s9 = const opensbi::SBI_TRAP_REGS_s9 * 8,
            sbi_trap_regs_offset_s10 = const opensbi::SBI_TRAP_REGS_s10 * 8,
            sbi_trap_regs_offset_s11 = const opensbi::SBI_TRAP_REGS_s11 * 8,
            sbi_trap_regs_offset_t3 = const opensbi::SBI_TRAP_REGS_t3 * 8,
            sbi_trap_regs_offset_t4 = const opensbi::SBI_TRAP_REGS_t4 * 8,
            sbi_trap_regs_offset_t5 = const opensbi::SBI_TRAP_REGS_t5 * 8,
            sbi_trap_regs_offset_t6 = const opensbi::SBI_TRAP_REGS_t6 * 8,
        )
    }
}
#[no_mangle]
#[inline(always)]
fn trap_save_info() {
    unsafe {
        asm!(
            "csrr t0, {csr_mcause}",
            "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_cause})(sp)",
            "csrr t0, {csr_mtval}",
            "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tval})(sp)",
            "sd zero, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tval2})(sp)",
            "sd zero, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tinst})(sp)",
            "li t0, 0",
            "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_gva})(sp)",
            csr_mcause = const opensbi::CSR_MCAUSE,
            csr_mtval = const opensbi::CSR_MTVAL,
            sbi_trap_regs_size = const 8 * opensbi::SBI_TRAP_REGS_last,
            sbi_trap_info_offset_cause = const 8 * opensbi::SBI_TRAP_INFO_cause,
            sbi_trap_info_offset_tval = const 8 * opensbi::SBI_TRAP_INFO_tval,
            sbi_trap_info_offset_tval2 = const 8 * opensbi::SBI_TRAP_INFO_tval2,
            sbi_trap_info_offset_tinst = const 8 * opensbi::SBI_TRAP_INFO_tinst,
            sbi_trap_info_offset_gva = const 8 * opensbi::SBI_TRAP_INFO_gva,
        )
    };
    clear_mdt_t0();
}
#[no_mangle]
#[inline(always)]
fn trap_call_c_routine() {
    unsafe {
        asm!("add a0, sp, zero", "call {sbi_trap_handler}", sbi_trap_handler = sym opensbi::sbi_trap_handler)
    }
}
#[no_mangle]
#[inline(always)]
fn trap_restore_general_regs_except_a0_t0() {
    unsafe {
        asm!(
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
            sbi_trap_regs_offset_sp = const opensbi::SBI_TRAP_REGS_sp * 8,
            sbi_trap_regs_offset_ra = const opensbi::SBI_TRAP_REGS_ra * 8,
            sbi_trap_regs_offset_gp = const opensbi::SBI_TRAP_REGS_gp * 8,
            sbi_trap_regs_offset_tp = const opensbi::SBI_TRAP_REGS_tp * 8,
            sbi_trap_regs_offset_t1 = const opensbi::SBI_TRAP_REGS_t1 * 8,
            sbi_trap_regs_offset_t2 = const opensbi::SBI_TRAP_REGS_t2 * 8,
            sbi_trap_regs_offset_s0 = const opensbi::SBI_TRAP_REGS_s0 * 8,
            sbi_trap_regs_offset_s1 = const opensbi::SBI_TRAP_REGS_s1 * 8,
            sbi_trap_regs_offset_a1 = const opensbi::SBI_TRAP_REGS_a1 * 8,
            sbi_trap_regs_offset_a2 = const opensbi::SBI_TRAP_REGS_a2 * 8,
            sbi_trap_regs_offset_a3 = const opensbi::SBI_TRAP_REGS_a3 * 8,
            sbi_trap_regs_offset_a4 = const opensbi::SBI_TRAP_REGS_a4 * 8,
            sbi_trap_regs_offset_a5 = const opensbi::SBI_TRAP_REGS_a5 * 8,
            sbi_trap_regs_offset_a6 = const opensbi::SBI_TRAP_REGS_a6 * 8,
            sbi_trap_regs_offset_a7 = const opensbi::SBI_TRAP_REGS_a7 * 8,
            sbi_trap_regs_offset_s2 = const opensbi::SBI_TRAP_REGS_s2 * 8,
            sbi_trap_regs_offset_s3 = const opensbi::SBI_TRAP_REGS_s3 * 8,
            sbi_trap_regs_offset_s4 = const opensbi::SBI_TRAP_REGS_s4 * 8,
            sbi_trap_regs_offset_s5 = const opensbi::SBI_TRAP_REGS_s5 * 8,
            sbi_trap_regs_offset_s6 = const opensbi::SBI_TRAP_REGS_s6 * 8,
            sbi_trap_regs_offset_s7 = const opensbi::SBI_TRAP_REGS_s7 * 8,
            sbi_trap_regs_offset_s8 = const opensbi::SBI_TRAP_REGS_s8 * 8,
            sbi_trap_regs_offset_s9 = const opensbi::SBI_TRAP_REGS_s9 * 8,
            sbi_trap_regs_offset_s10 = const opensbi::SBI_TRAP_REGS_s10 * 8,
            sbi_trap_regs_offset_s11 = const opensbi::SBI_TRAP_REGS_s11 * 8,
            sbi_trap_regs_offset_t3 = const opensbi::SBI_TRAP_REGS_t3 * 8,
            sbi_trap_regs_offset_t4 = const opensbi::SBI_TRAP_REGS_t4 * 8,
            sbi_trap_regs_offset_t5 = const opensbi::SBI_TRAP_REGS_t5 * 8,
            sbi_trap_regs_offset_t6 = const opensbi::SBI_TRAP_REGS_t6 * 8,
        )
    }
}

#[no_mangle]
#[inline(always)]
fn trap_restore_mepc_status() {
    unsafe {
        asm!(
            "ld t0, {sbi_trap_regs_offset_mstatus}(a0)",
            "csrw {csr_mstatus}, t0",
            "ld t0, {sbi_trap_regs_offset_mepc}(a0)",
            "csrw {csr_mepc}, t0",
            csr_mstatus = const opensbi::CSR_MSTATUS,
            csr_mepc= const opensbi::CSR_MEPC,
            sbi_trap_regs_offset_mepc = const opensbi::SBI_TRAP_REGS_mepc * 8,
            sbi_trap_regs_offset_mstatus= const opensbi::SBI_TRAP_REGS_mstatus * 8,
        )
    }
}
#[no_mangle]
#[inline(always)]
fn trap_restore_a0_t0() {
    unsafe {
        asm!(
            "ld t0, {sbi_trap_regs_offset_t0}(a0)",
            "ld a0, {sbi_trap_regs_offset_a0}(a0)",
            sbi_trap_regs_offset_t0 = const opensbi::SBI_TRAP_REGS_t0 * 8,
            sbi_trap_regs_offset_a0 = const opensbi::SBI_TRAP_REGS_a0 * 8,
        )
    }
}

#[no_mangle]
#[inline(always)]
pub fn clear_mdt_t0() {
    unsafe {
        asm!(
            "li t0, 0x40000000000",
            "csrc {csr_mstatus}, t0",
            csr_mstatus = const opensbi::CSR_MSTATUS,
        )
    }
}
