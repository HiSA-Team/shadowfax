/*
 * CoVE handler module. In this module, we provide CoVH and SUPD extension trap handling.
 * The handling is structured as follows:
 * - entry: the context is saved to the TEE_SCRATCH and calls the handler
 * - handler: the function which handles the interrupt and prepare the context switch. Returns the
 * address of the Context to be restored
 * - exit: restores the Context prepared by the handler
 *
 * While the entry is separed, the exit is shared across the SUPD and COVH since the operations are
 * the same. The entry tells the exit if it needs to restore the PMP using a0 register (0 don't
 * restore)
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

use core::mem::offset_of;

use common::sbi::{
    SBI_COVH_CONVERT_PAGES, SBI_COVH_EXT_ID, SBI_COVH_GET_TSM_INFO,
    SBI_EXT_SUPD_GET_ACTIVE_DOMAINS, SBI_SUPD_EXT_ID,
};

use crate::{_tee_stack_top, context::Context, opensbi, state::STATE};

macro_rules! cove_unpack_fid {
    ($fid:expr) => {
        (($fid >> 26) & 0x3F, $fid & 0xFFFF)
    };
}

// 8K scratch memory
pub const TEE_SCRATCH_SIZE: usize = 0x2000;

#[unsafe(naked)]
pub fn tee_handler_entry() -> ! {
    core::arch::naked_asm!(
    // calculate new stack pointer for tee handling. To do so, we use the mscratch and adapt to
    // the opensbi scartch memory layout.
    // This block needs:
    // - a7 as base pointer as we assume it as CoVE ID
    // - t0 as arithemtic register to calculate the offset
    "
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
        //csrr t0,senvcfg
        //sd t0, 38*8(sp)
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

        // call tee handler
        la sp, {tee_stack}
        add a0, a6, zero
        call {tee_handler}

        // restore the target supervisor domain
        add sp, a0, zero
        li a0, 1
        j {tee_handler_exit}
        ",
        tee_stack = sym _tee_stack_top,
        covh_ext_id = const SBI_COVH_EXT_ID,
        context_size= const size_of::<Context>(),
        scratch_size = const TEE_SCRATCH_SIZE,
        sbi_scratch_tmp0_offset = const offset_of!(opensbi::sbi_scratch, tmp0),
        tee_handler = sym covh_handler,
        tee_handler_exit = sym tee_handler_exit
    )
}

/// Handle the CoVH call:
/// - Unlock the state;
/// - Find out if it is a TEECALL or a TEERET
/// - Find the destination context address
/// - Return the destination address
///
/// The `domain.active` field of a TSM encodes the src supervisor domain which must be preserved by
/// the TSM in a TEERET.
#[no_mangle]
#[inline(never)]
extern "C" fn covh_handler(fid: usize) -> usize {
    // unlock the state
    let mut guard = STATE.lock();
    let state = guard.get_mut().unwrap();

    // scratch context base pointer
    let scratch_start = &raw const _tee_stack_top as *const u8 as usize;
    let base_ctx = scratch_start - (TEE_SCRATCH_SIZE + size_of::<Context>());
    let scratch_ctx = base_ctx as *mut Context;

    let (dst_id, fid) = cove_unpack_fid!(fid);
    let tsm = state.tsms.iter_mut().find(|d| d.id == dst_id);

    // TEECALL
    if let Some(tsm) = tsm {
        let src_id = state.current_id;
        // check if the domain is trusted. If not just return an error to the caller
        if !tsm.is_trusted(src_id) {
            unsafe {
                (*scratch_ctx).regs[10] = usize::MAX;
                (*scratch_ctx).regs[11] = 0;
                // increment mepc to avoid loop
                (*scratch_ctx).mepc += 4;
            }
            return base_ctx;
        }
        // We need to store the calling context into the right structure
        let caller_ctx_addr = base_ctx - (src_id + 1) * size_of::<Context>();
        let caller_ctx = caller_ctx_addr as *mut Context;
        unsafe {
            core::ptr::copy_nonoverlapping(scratch_ctx, caller_ctx, 1);
        }

        let tsm_ctx = tsm.context_addr as *mut Context;

        // we need to preserve all a0-a4 and a6--a7 registers.
        unsafe {
            // a0 is the 10th general purpose register
            // a7 is the 17th general purpose register
            for i in 10..18 {
                (*tsm_ctx).regs[i] = (*caller_ctx).regs[i];
            }

            // save the caller into a6 register, but we must preserve the EID.
            // The caller id must be saved in bits [31:26]
            let eid = (*tsm_ctx).regs[16] & 0xFFFF;
            (*tsm_ctx).regs[16] = ((src_id & 0x3F) << 26) | eid;

            // save the TSM state into t0
            (*tsm_ctx).regs[5] = tsm.state_addr;

            // save the caller context address into TSM context
            (*tsm_ctx).caller_ctx = caller_ctx_addr;
        }

        // Perform operations to allow the specific functionality
        match fid {
            // For sbi_covh_get_tsm_info we need to give the TSM access to the memory space
            // where he will write the tsm_info struct (a0) for the necessary size (a1).
            SBI_COVH_GET_TSM_INFO => {
                let addr = unsafe { (*tsm_ctx).regs[10] };
                let size = unsafe { (*tsm_ctx).regs[11] };

                let slot = 7;

                // Build the CFG byte for TOR + RW (not locked)
                let range = riscv::register::Range::TOR as usize;
                let perm = riscv::register::Permission::RW as usize;
                let locked = false as usize;
                let cfg_byte = (locked << 7) | (range << 3) | (perm);

                // Mask out old byte for slot 1 in pmpcfg0
                let byte_mask = 0xFF << (slot * 8);

                unsafe {
                    (*tsm_ctx).pmpaddr[slot - 1] = addr >> 2;
                    (*tsm_ctx).pmpaddr[slot] = (addr + size) >> 2;

                    (*tsm_ctx).pmpcfg &= !byte_mask;
                    (*tsm_ctx).pmpcfg |= cfg_byte << (slot * 8);
                }
            }
            SBI_COVH_CONVERT_PAGES => {
                let start_addr = unsafe { (*tsm_ctx).regs[10] };
                let num_pages = unsafe { (*tsm_ctx).regs[11] };
                let slot = tsm.next_pmp_slot;

                if slot < 6 {
                    // TODO: remove these pages from the caller PMP
                    // setup pmp entry for the block of pages requested
                    let end_addr = start_addr + num_pages * 4096;

                    let size = (end_addr - start_addr).next_power_of_two();
                    let base = start_addr & !(size - 1);

                    let k = size.trailing_zeros() as usize;
                    let ones = (1 << (k - 3)) - 1;

                    let pmpaddr = ((base >> 2) as usize) | ones;
                    let locked = false;
                    let range = riscv::register::Range::NAPOT;
                    let permission = riscv::register::Permission::RWX;
                    let byte =
                        (locked as usize) << 7 | (range as usize) << 3 | (permission as usize);
                    let pmpcfg = byte << (8 * slot);

                    unsafe {
                        (*tsm_ctx).pmpcfg |= pmpcfg;
                        (*tsm_ctx).pmpaddr[slot] = pmpaddr;
                    }

                    tsm.next_pmp_slot += 1;
                } else {
                    // no more pmp slot available, return an error
                    unsafe {
                        (*scratch_ctx).regs[10] = usize::MAX;
                        (*scratch_ctx).regs[11] = 0;
                        // increment mepc to avoid loop
                        (*scratch_ctx).mepc += 4;
                    }
                    return base_ctx;
                }
            }
            _ => {}
        }
        // mark the new tsm as the current domain
        state.current_id = tsm.id;
        return tsm.context_addr;
    }

    // TEERET
    // Retrive the TSM ID as the current running
    // We don't need to store the calling context since we are implementing the
    // non interruptible TSM. We need a0 and a1 registers to deliver the result
    let tsm = state
        .tsms
        .iter()
        .find(|t| t.id == state.current_id)
        .unwrap();

    let tsm_ctx = tsm.context_addr as *mut Context;
    let caller_ctx = unsafe { (*tsm_ctx).caller_ctx as *mut Context };

    unsafe {
        (*caller_ctx).regs[10] = (*scratch_ctx).regs[10];
        (*caller_ctx).regs[11] = (*scratch_ctx).regs[11];
        (*caller_ctx).regs[16] = (*scratch_ctx).regs[16];
        // increment mepc to avoid loop
        (*caller_ctx).mepc += 4;
    }
    // Perform operations to cleanup specific to the functionality
    match fid {
        // Reset the PMP address to the shared memory
        SBI_COVH_GET_TSM_INFO => unsafe {
            let slot = 7;
            let byte_mask = 0xFF << (slot * 8);
            (*tsm_ctx).pmpaddr[slot - 1] = 0;
            (*tsm_ctx).pmpaddr[slot] = 0;
            (*tsm_ctx).pmpcfg &= !byte_mask;
        },
        _ => {}
    }
    state.current_id = dst_id;
    return unsafe { (*tsm_ctx).caller_ctx };
}

#[unsafe(naked)]
pub fn supd_handler_entry() -> ! {
    core::arch::naked_asm!(
    "
        la a7, {tee_stack}
        li t0, {scratch_size}
        add t0, t0, {context_size}
        sub a7, a7, t0
        sd sp, 8*2(a7)
        add sp, a7, zero
        // restore a7 and t0 and swap back the mscratch
        la a7, {supd_ext_id}
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
        //csrr t0,senvcfg
        //sd t0, 38*8(sp)
        // sd t0, 39*8(sp)
        // csrr scontext, t0
        csrr t0, mepc
        sd t0, 40*8(sp)
    ",
    "
        la sp, {tee_stack}
        add a0, a6, zero
        call {handler}

        add sp, a0, zero
        li a0, 0
        j {tee_handler_exit}
    ",
        tee_stack = sym _tee_stack_top,
        supd_ext_id = const SBI_SUPD_EXT_ID,
        context_size= const size_of::<Context>(),
        scratch_size = const TEE_SCRATCH_SIZE,
        sbi_scratch_tmp0_offset = const offset_of!(opensbi::sbi_scratch, tmp0),
        handler = sym supd_handler,
        tee_handler_exit = sym tee_handler_exit
    )
}

fn supd_handler(fid: usize) -> usize {
    let mut guard = STATE.lock();
    let state = guard.get_mut().unwrap();
    let scratch_addr = &raw const _tee_stack_top as *const u8 as usize;
    let dst_addr = scratch_addr - (TEE_SCRATCH_SIZE + size_of::<Context>());
    let dst_ctx = dst_addr as *mut Context;

    if fid == SBI_EXT_SUPD_GET_ACTIVE_DOMAINS {
        // root supervisor domain is mandatory
        let mut ret: usize = 1;
        for d in state.tsms.iter() {
            ret |= 1 << d.id;
        }
        unsafe {
            (*dst_ctx).regs[10] = 0;
            (*dst_ctx).regs[11] = ret;
        }
    } else {
        unsafe {
            (*dst_ctx).regs[10] = usize::MAX - 1;
            (*dst_ctx).regs[11] = 0;
        }
    }

    unsafe {
        (*dst_ctx).mepc += 4;
    }

    return dst_addr;
}

#[unsafe(naked)]
fn tee_handler_exit() -> ! {
    core::arch::naked_asm!(
        "
            ld zero, 0(sp)
            ld ra, 1*8(sp)
            ld gp, 3*8(sp)
            ld tp, 4*8(sp)
            ld t1, 6*8(sp)
            ld t2, 7*8(sp)
            ld s0, 8*8(sp)
            ld s1, 9*8(sp)
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
            //ld t0, 38*8(sp)
            //csrw senvcfg, t0
            // ld t0, 39*8(sp)
            // csrw scontext, t0
            ld t0, 40*8(sp)
            csrw mepc, t0
        ",
        // restore pmp
        "
            // check if we need to restore the PMP
            beqz a0, 1f
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

            ld t0, 41*8(sp)
            csrw pmpcfg0, t0

            fence
            fence.i

            1:
            // restore t0, a0, sp
            ld t0, 5*8(sp)
            ld a0, 10*8(sp)
            ld sp, 2*8(sp)
        ",
        "
            mret
        ",
    )
}
