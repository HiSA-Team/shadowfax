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

use crate::{
    _tee_scratch_start, opensbi,
    sbi::SBI_COVH_GET_TSM_INFO,
    shadowfax_core::state::{Context, TsmType, STATE},
};
use core::mem::offset_of;

macro_rules! cove_unpack_fid {
    ($fid:expr) => {
        (($fid >> 26) & 0x3F, $fid & 0xFFFF)
    };
}

pub const TEE_SCRATCH_SIZE: usize = 0xF000;

/// The main trap handler function that orchestrates the saving and restoring of registers.
/// The handler verifies if the trap is a TEECALL/TEERESUME or a TEERET and handles it with custom
/// logic.
#[align(4)]
#[unsafe(naked)]
pub unsafe extern "C" fn handler() -> ! {
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
            bnez t0, 1f
            ld t0, {sbi_scratch_tmp0_offset}(tp)
            csrrw tp, mscratch, tp
            j {tee_handler}
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
        covh_ext_id = const 0x434F5648 as usize,
        tee_handler = sym tee_handler_entry,

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

#[unsafe(naked)]
fn tee_handler_entry() -> ! {
    core::arch::naked_asm!(
    // calculate new stack pointer for tee handling. To do so, we use the mscratch and adapt to
    // the opensbi scartch memory layout.
    // This block needs:
    // - a7 as base pointer as we assume it as CoVE ID
    // - t0 as arithemtic register to calculate the offset
    "
        csrrw tp, mscratch, tp
        sd t0, {sbi_scratch_tmp0_offset}(tp)
        la a7, {tee_stack}
        li t0, {scratch_size}
        add t0, t0, {context_size}
        sub a7, a7, t0
        sd sp, 8*2(a7)
        add sp, a7, zero
        // restore a7 and t0 and swap back the mscratch
        la a7, {covh_ext_id}
        ld t0, {sbi_scratch_tmp0_offset}(tp)
        csrrw tp, mscratch, tp
        ",
    // save gprs
    "
        sd x0, 8 * 0 (sp)
        sd x1, 8 * 1 (sp)
        sd x3, 8 * 3 (sp)
        sd x4, 8 * 4 (sp)
        sd x5, 8 * 5 (sp)
        sd x6, 8 * 6 (sp)
        sd x7, 8 * 7 (sp)
        sd x8, 8 * 8 (sp)
        sd x9, 8 * 9 (sp)
        sd x10, 8 * 10 (sp)
        sd x11, 8 * 11 (sp)
        sd x12, 8 * 12 (sp)
        sd x13, 8 * 13 (sp)
        sd x14, 8 * 14 (sp)
        sd x15, 8 * 15 (sp)
        sd x16, 8 * 16 (sp)
        sd x17, 8 * 17 (sp)
        sd x18, 8 * 18 (sp)
        sd x19, 8 * 19 (sp)
        sd x20, 8 * 20 (sp)
        sd x21, 8 * 21 (sp)
        sd x22, 8 * 22 (sp)
        sd x23, 8 * 23 (sp)
        sd x24, 8 * 24 (sp)
        sd x25, 8 * 25 (sp)
        sd x26, 8 * 26 (sp)
        sd x27, 8 * 27 (sp)
        sd x28, 8 * 28 (sp)
        sd x29, 8 * 29 (sp)
        sd x30, 8 * 30 (sp)
        sd x31, 8 * 31 (sp)
        ",
    // save csrs
    "
        csrr t0, sstatus
        sd t0, 32*8(sp)
        csrr t0, stvec
        sd t0, 33*8(sp)
        csrr t0, sip
        sd t0, 34*8(sp)
        csrr  t0,scounteren
        sd t0, 35*8(sp)
        csrr  t0, sscratch
        sd t0, 36*8(sp)
        csrr t0, satp
        sd t0, 37*8(sp)
        csrr t0,senvcfg
        sd t0, 38*8(sp)
        // sd t0, 39*8(sp)
        // csrr scontext, t0
        csrr t0, mepc
        sd t0, 40*8(sp)
        ",
    // save pmp config
    "
        csrr t0, pmpcfg0
        sd t0, 41*8(sp)
        csrr t0, pmpaddr0
        sd t0, 42*8(sp)
        csrr t0, pmpaddr1
        sd t0, 43*8(sp)
        csrr t0, pmpaddr2
        sd t0, 44*8(sp)
        csrr t0, pmpaddr3
        sd t0, 45*8(sp)
        csrr t0, pmpaddr4
        sd t0, 46*8(sp)
        csrr t0, pmpaddr5
        sd t0, 47*8(sp)
        csrr t0, pmpaddr6
        sd t0, 48*8(sp)
        csrr t0, pmpaddr7
        sd t0, 49*8(sp)
        csrr t0, pmpaddr8
        sd t0, 50*8(sp)
        csrr t0, pmpaddr9
        sd t0, 51*8(sp)
        csrr t0, pmpaddr10
        sd t0, 52*8(sp)
        csrr t0, pmpaddr11
        sd t0, 53*8(sp)
        csrr t0, pmpaddr12
        sd t0, 54*8(sp)
        csrr t0, pmpaddr13
        sd t0, 55*8(sp)
        csrr t0, pmpaddr14
        sd t0, 56*8(sp)
        csrr t0, pmpaddr15
        sd t0, 57*8(sp)
        la sp, {tee_stack}
        add a0, a6, zero
        call {tee_handler}
        ",
        tee_stack = sym _tee_scratch_start,
        covh_ext_id = const 0x434F5648 as usize,
        context_size= const size_of::<Context>(),
        scratch_size = const TEE_SCRATCH_SIZE,
        sbi_scratch_tmp0_offset = const offset_of!(opensbi::sbi_scratch, tmp0),
        tee_handler = sym tee_handler
    )
}

#[no_mangle]
#[inline(never)]
extern "C" fn tee_handler(fid: usize) -> ! {
    // unlock the state
    let mut state_guard = STATE.lock();
    let state = state_guard.get_mut().unwrap();

    let (dst_domain_id, fid) = cove_unpack_fid!(fid);
    let active_domain = state.domains.iter().find(|d| d.active != 0).unwrap();
    let active_domain_id = active_domain.id;
    let dst_domain_type = state.domains[dst_domain_id].tsm_type.clone();
    let scratch_addr = &raw const _tee_scratch_start as *const u8 as usize;
    let scratch_ctx = (scratch_addr - (TEE_SCRATCH_SIZE + size_of::<Context>())) as *mut Context;

    // understand if we are in a TEECALL or a TEERET. To do so we need to check the target
    // domain (which is assumed to be a confidential domain). If the target domain is the same as
    // the active one, it means that this call comes from a TSM and it is a TEERET, otherwise it is
    // a TEECALL.
    // The outcome of this block is the address of the domain to be restored.
    let dst_addr = if active_domain_id == dst_domain_id {
        // TEERET
        // We don't need to store the calling context since we are implementing the
        // non interruptible TSM. We need a0 and a1 registers to deliver the result
        //
        // We need to retrieve the original calling context
        let src_id = (active_domain.active & !(1 << dst_domain_id)).trailing_zeros() as usize;
        let dst_addr = scratch_addr
            - (TEE_SCRATCH_SIZE + size_of::<Context>())
            - (src_id + 1) * size_of::<Context>();
        let dst_ctx = dst_addr as *mut Context;
        unsafe {
            (*dst_ctx).regs[10] = (*scratch_ctx).regs[10];
            (*dst_ctx).regs[11] = (*scratch_ctx).regs[11];
            // increment mepc to avoid loop
            (*dst_ctx).mepc += 4;
        }
        // Perform operations to cleanup specific to the functionality
        match fid {
            // Reset the PMP address to the shared memory
            SBI_COVH_GET_TSM_INFO => {
                let tsm_ctx = (scratch_addr
                    - (TEE_SCRATCH_SIZE + size_of::<Context>())
                    - (dst_domain_id + 1) * size_of::<Context>())
                    as *mut Context;

                unsafe {
                    (*tsm_ctx).pmpaddr[2] = 0;
                    (*tsm_ctx).pmpaddr[1] = 0;
                    (*dst_ctx).pmpcfg &= !0xFF << (2 * 8);
                }
            }
            _ => {}
        }
        state.domains[active_domain_id].active = 0;
        state.domains[src_id].active = 1 << src_id;
        dst_addr
    } else {
        // TEECALL
        // If the target domain is not a TSM, we will just respond to the src domain
        // with an error. In this case we don't do the context switch into the TSM since there is
        // not one. A malicious supervisor domain could attempt to get into a non TEE-aware
        // OS. So we change the dst_addr to the src domain.
        match dst_domain_type {
            TsmType::None => {
                let dst_addr = scratch_addr - (TEE_SCRATCH_SIZE + size_of::<Context>());

                let dst_ctx = dst_addr as *mut Context;
                unsafe {
                    (*dst_ctx).regs[10] = usize::MAX;
                    (*dst_ctx).regs[11] = 0;
                    // increment mepc to avoid loop
                    (*dst_ctx).mepc += 4;
                }
                dst_addr
            }
            _ => {
                // We need to store the calling context into the right structure
                let src_ctx = (scratch_addr
                    - (TEE_SCRATCH_SIZE + size_of::<Context>())
                    - (active_domain_id + 1) * size_of::<Context>())
                    as *mut Context;
                unsafe {
                    core::ptr::copy_nonoverlapping(scratch_ctx, src_ctx, 1);
                }
                let dst_addr = scratch_addr
                    - (TEE_SCRATCH_SIZE + size_of::<Context>())
                    - (dst_domain_id + 1) * size_of::<Context>();

                let dst_ctx = dst_addr as *mut Context;
                unsafe {
                    // we need to preserve all a0-a7 registers.
                    // Easier (maybe to help prefetch to store everything and to delete a5)
                    for i in 10..18 {
                        (*dst_ctx).regs[i] = (*src_ctx).regs[i];
                    }
                }

                state.domains[active_domain_id].active = 0;
                state.domains[dst_domain_id].active =
                    (1 << active_domain_id) | (1 << dst_domain_id);

                // Perform operations to allow the specific functionality
                match fid {
                    // For sbi_covh_get_tsm_info we need to give the TSM access to the memory space
                    // where he will write the tsm_info struct (a0) for the necessary size (a1).
                    SBI_COVH_GET_TSM_INFO => {
                        let addr = unsafe { (*dst_ctx).regs[10] };
                        let size = unsafe { (*dst_ctx).regs[11] };

                        let slot = 2;

                        // Build the CFG byte for TOR + RW (not locked)
                        let range = riscv::register::Range::TOR as usize;
                        let perm = riscv::register::Permission::RW as usize;
                        let locked = false as usize;
                        let cfg_byte = (locked << 7) | (range << 3) | (perm);

                        // Mask out old byte for slot 1 in pmpcfg0
                        let byte_mask = 0xff << (slot * 8);

                        unsafe {
                            (*dst_ctx).pmpaddr[slot - 1] = addr >> 2;
                            (*dst_ctx).pmpaddr[slot] = (addr + size) >> 2;

                            (*dst_ctx).pmpcfg &= !byte_mask;
                            (*dst_ctx).pmpcfg |= cfg_byte << (slot * 8);
                        }
                    }
                    _ => {}
                }
                dst_addr
            }
        }
    };

    // release the lock
    drop(state_guard);

    // restore target domain context
    unsafe {
        core::arch::asm!(
        "
        mv sp, {target_domain}
        j {tee_handler_exit}

        ",
        target_domain  = in(reg) dst_addr,
        tee_handler_exit = sym tee_handler_exit,
        options(nostack, noreturn)
        )
    }
}

#[unsafe(naked)]
fn tee_handler_exit() -> ! {
    core::arch::naked_asm!(
        "
            ld zero, 0(sp)
            ld ra, 1*8(sp)
            ld gp, 3*8(sp)
            ld tp, 4*8(sp)
            ld t0, 5*8(sp)
            ld t1, 6*8(sp)
            ld t2, 7*8(sp)
            ld s0, 8*8(sp)
            ld s1, 9*8(sp)
            ld a0, 10*8(sp)
            ld a1, 11*8(sp)
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
        // restore CSRs
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
        // restore pmp
        "
            ld t0, 42*8(sp)
            csrw pmpaddr0, t0
            ld t0, 43*8(sp)
            csrw pmpaddr1, t0
            ld t0, 44*8(sp)
            csrw pmpaddr2, t0
            ld t0, 45*8(sp)
            csrw pmpaddr3, t0
            ld t0, 46*8(sp)
            csrw pmpaddr4, t0
            ld t0, 47*8(sp)
            csrw pmpaddr5, t0
            ld t0, 48*8(sp)
            csrw pmpaddr6, t0
            ld t0, 49*8(sp)
            csrw pmpaddr7, t0
            fence
            fence.i
            ld t0, 41*8(sp)
            csrw pmpcfg0, t0
        ",
        "
            // restore t0 and sp
            ld t0, 5*8(sp)
            ld sp, 2*8(sp)
            mret
        ",
    )
}
