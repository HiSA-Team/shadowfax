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

use crate::{_tee_scratch_start, context::Context, domain::TsmType, opensbi, state::STATE};

pub const COVH_EXT_ID: usize = 0x434F5648;
pub const SBI_COVH_GET_TSM_INFO: usize = 0;

pub const SUPD_EXT_ID: usize = 0x53555044;
pub const SBI_EXT_SUPD_GET_ACTIVE_DOMAINS: usize = 0;

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
        tee_stack = sym _tee_scratch_start,
        covh_ext_id = const COVH_EXT_ID,
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

    let (dst_id, fid) = cove_unpack_fid!(fid);
    let active_domain = state.domains.iter().find(|d| d.active != 0).unwrap();
    let src_id = active_domain.id;
    let dst_type = state.domains[dst_id].tsm_type.clone();

    // scratch context base pointer
    let scratch_start = &raw const _tee_scratch_start as *const u8 as usize;
    let base_ctx = scratch_start - (TEE_SCRATCH_SIZE + size_of::<Context>());
    let scratch_ctx = base_ctx as *mut Context;

    // understand if we are in a TEECALL or a TEERET. To do so we need to check the target
    // domain (which is assumed to be a confidential domain). If the target domain is the same as
    // the active one, it means that this call comes from a TSM and it is a TEERET, otherwise it is
    // a TEECALL.
    // The outcome of this block is the address of the domain to be restored.
    if src_id == dst_id {
        // TEERET
        // We don't need to store the calling context since we are implementing the
        // non interruptible TSM. We need a0 and a1 registers to deliver the result
        //
        // We need to retrieve the original calling context
        let target_id = (active_domain.active & !(1 << dst_id)).trailing_zeros() as usize;
        let dst_addr = base_ctx - (target_id + 1) * size_of::<Context>();
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
                let tsm_ctx = (scratch_start
                    - (TEE_SCRATCH_SIZE + size_of::<Context>())
                    - (dst_id + 1) * size_of::<Context>())
                    as *mut Context;

                unsafe {
                    (*tsm_ctx).pmpaddr[2] = 0;
                    (*tsm_ctx).pmpaddr[1] = 0;
                    (*dst_ctx).pmpcfg &= !0xFF << (2 * 8);
                }
            }
            _ => {}
        }
        state.domains[src_id].active = 0;
        state.domains[target_id].active = 1 << target_id;
        return dst_addr;
    }
    // TEECALL
    // If the target domain is not a TSM, we will just respond to the src domain
    // with an error. In this case we don't do the context switch into the TSM since there is
    // not one. A malicious supervisor domain could attempt to get into a non TEE-aware
    // OS. So we change the dst_addr to the src domain.
    let dst_domain = &state.domains[dst_id];
    if matches!(dst_type, TsmType::None)
        || !active_domain.is_trusted(dst_id)
        || !dst_domain.is_trusted(src_id)
    {
        unsafe {
            (*scratch_ctx).regs[10] = usize::MAX;
            (*scratch_ctx).regs[11] = 0;
            // increment mepc to avoid loop
            (*scratch_ctx).mepc += 4;
        }
        return base_ctx;
    }
    // first check if there is a trust relationship between the two domains
    // We need to store the calling context into the right structure
    let src_ctx = (base_ctx - (src_id + 1) * size_of::<Context>()) as *mut Context;
    unsafe {
        core::ptr::copy_nonoverlapping(scratch_ctx, src_ctx, 1);
    }
    let dst_addr = base_ctx - (dst_id + 1) * size_of::<Context>();

    let dst_ctx = dst_addr as *mut Context;
    unsafe {
        // we need to preserve all a0-a7 registers.
        for i in 10..18 {
            (*dst_ctx).regs[i] = (*src_ctx).regs[i];
        }
    }

    // mark the src domain inactive and the target TSM as "in communication with the src domain"
    state.domains[src_id].active = 0;
    state.domains[dst_id].active = (1 << src_id) | (1 << dst_id);

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
        tee_stack = sym _tee_scratch_start,
        supd_ext_id = const SUPD_EXT_ID,
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
    let scratch_addr = &raw const _tee_scratch_start as *const u8 as usize;
    let dst_addr = scratch_addr - (TEE_SCRATCH_SIZE + size_of::<Context>());
    let dst_ctx = dst_addr as *mut Context;

    if fid == SBI_EXT_SUPD_GET_ACTIVE_DOMAINS {
        let mut ret: usize = 0;
        for d in state.domains.iter() {
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
