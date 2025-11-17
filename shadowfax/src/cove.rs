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
    COVH_DEFAULT_PAGE_SIZE, SBI_COVH_CONVERT_PAGES, SBI_COVH_EXT_ID, SBI_COVH_GET_TSM_INFO,
    SBI_EXT_SUPD_GET_ACTIVE_DOMAINS, SBI_SUPD_EXT_ID,
};

use crate::{
    _tee_stack_top,
    context::Context,
    domain::{build_pmp_configuration_registers, MemoryRegion},
    opensbi,
    state::STATE,
};

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
    "
        // call tee handler
        la sp, {tee_stack}
        add a0, a6, zero
        call {tee_handler}

        // restore the target supervisor domain
        add sp, a0, zero
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

    let (dst_id, fid) = cove_unpack_fid!(fid);
    let domain = state.domains.get_mut(dst_id);

    // Scratch space
    let scratch_start = &raw const _tee_stack_top as *const u8 as usize;
    let base_ctx = scratch_start - (TEE_SCRATCH_SIZE + size_of::<Context>());
    let scratch_ctx = base_ctx as *mut Context;

    // Invalid domain id, go back with an error
    if domain.is_none() {
        return unsafe { return_error(base_ctx, -1) };
    }

    // Get destination domain
    let domain = domain.unwrap();

    // TEECALL
    if domain.state_addr.is_some() {
        let domain_ctx = domain.context_addr as *mut Context;
        // TODO: get current domain
        let src_id = 2;
        // check if the domain is trusted. If not just return an error to the caller
        if !domain.is_trusted(src_id) {
            return unsafe { return_error(base_ctx, -1) };
        }
        // We need to store the calling context into the right structure
        let caller_ctx_addr = base_ctx - (src_id) * size_of::<Context>();
        let caller_ctx = caller_ctx_addr as *mut Context;
        unsafe {
            core::ptr::copy_nonoverlapping(scratch_ctx, caller_ctx, 1);
        }

        // we need to preserve all a0-a7 registers as they are input of the ecall
        unsafe {
            // a0 is the 10th general purpose register
            // a7 is the 17th general purpose register
            for i in 10..18 {
                (*domain_ctx).regs[i] = (*caller_ctx).regs[i];
            }

            // Save the caller id into a6 register, but we must preserve the EID. This is used for
            // the TEERET
            // The caller id must be saved in bits [31:26]
            let eid = (*domain_ctx).regs[16] & 0xFFFF;
            (*domain_ctx).regs[16] = ((src_id) << 26) | eid;

            // save the domain state into t0
            (*domain_ctx).regs[5] = domain.state_addr.unwrap();

            // save the caller context address into domain context
            (*domain_ctx).caller_ctx = caller_ctx_addr;
        }

        // Perform operations to allow the specific functionality
        match fid {
            // For sbi_covh_get_domain_info we need to give the TSM access to the memory space
            // where he will write the domain_info struct (a0) for the necessary size (a1).
            SBI_COVH_GET_TSM_INFO => {
                let base_addr = unsafe { (*domain_ctx).regs[10] };
                let size = unsafe { (*domain_ctx).regs[11] };

                // Base address must be page aligned, we cannot exceed number of available pmp
                // registers
                assert!(base_addr % COVH_DEFAULT_PAGE_SIZE == 0);
                assert!(domain.memory_regions.len() < 8);

                let order = if (size & (size - 1)) == 0 {
                    size.trailing_zeros()
                } else {
                    size.next_power_of_two().trailing_zeros()
                }
                .max(3);

                domain.memory_regions.push(MemoryRegion {
                    base_addr,
                    order,
                    mmio: false,
                    permissions: 0x3f,
                });
            }
            SBI_COVH_CONVERT_PAGES => {
                let base_addr = unsafe { (*domain_ctx).regs[10] };
                let num_pages = unsafe { (*domain_ctx).regs[11] };

                // Base address must be page aligned, we cannot exceed number of available pmp
                // registers
                assert!(base_addr % COVH_DEFAULT_PAGE_SIZE == 0);
                assert!(domain.memory_regions.len() < 8);

                let order = (num_pages * COVH_DEFAULT_PAGE_SIZE).trailing_zeros();

                domain.memory_regions.push(MemoryRegion {
                    base_addr,
                    order,
                    mmio: false,
                    permissions: 0x3f,
                });
            }
            _ => {}
        }
        unsafe {
            let ret = opensbi::sbi_domain_change_active(dst_id as u32);
            assert!(ret == 0);
        }
        program_pmp_from_regions(&domain.memory_regions);
        return domain.context_addr;
    }

    // TEERET
    // We don't need to store the calling context since we are implementing the
    // non reentrant TSM. We need a0 and a1 registers to deliver the result

    // Restore the original TSM id
    // TODO make this dynamic
    let tsmid = 1;

    unsafe {
        let domain_ctx = domain.context_addr as *mut Context;
        let eid = (*scratch_ctx).regs[16] & 0xFFFF;
        (*domain_ctx).regs[10] = (*scratch_ctx).regs[10];
        (*domain_ctx).regs[11] = (*scratch_ctx).regs[11];
        (*domain_ctx).regs[16] = (tsmid << 26) | eid;
        // increment mepc to avoid loop
        (*domain_ctx).mepc += 4;
    }

    // Perform operations to cleanup specific to the functionality
    match fid {
        // Remove the last memory region
        SBI_COVH_GET_TSM_INFO => {}
        SBI_COVH_CONVERT_PAGES => {}
        _ => {}
    }
    unsafe {
        let ret = opensbi::sbi_domain_change_active(dst_id as u32);
        assert!(ret == 0);
    }
    program_pmp_from_regions(&domain.memory_regions);
    return domain.context_addr;
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
        for i in 0..state.domains.len() {
            ret |= 1 << i;
        }

        unsafe {
            (*dst_ctx).regs[10] = 0;
            (*dst_ctx).regs[11] = ret;
            (*dst_ctx).mepc += 4;
            return dst_addr;
        }
    }
    return unsafe { return_error(dst_addr, -1) };
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
        // restore t0, a0, sp
        "
            ld t0, 5*8(sp)
            ld a0, 10*8(sp)
            ld sp, 2*8(sp)
            mret
        ",
    )
}

// Encode an error code to the a0 register of the calling context and increment mepc
unsafe fn return_error(ctx_addr: usize, code: isize) -> usize {
    let ctx = ctx_addr as *mut Context;

    (*ctx).regs[10] = code as usize;
    (*ctx).regs[11] = 0;
    (*ctx).mepc += 4;

    return ctx_addr;
}

// Program the PMP as stated in 3.7 in Privileged ISA
fn program_pmp_from_regions(regions: &[MemoryRegion]) {
    for (i, r) in regions.iter().enumerate() {
        let ones = (1 << (r.order - 3)) - 1;
        let range = riscv::register::Range::NAPOT as usize;
        let permission = riscv::register::Permission::RWX as usize;

        // This should be a byte and be shifted by index
        let pmpcfg = ((0) << 7 | (range) << 3 | (permission)) & 0xFF;
        let pmpaddr = ((r.base_addr >> 2) as usize) | ones as usize;

        write_pmpaddr(i, pmpaddr);
        write_pmpcfg(i, pmpcfg);
    }
}

fn write_pmpaddr(index: usize, val: usize) {
    unsafe {
        match index {
            0 => core::arch::asm!("csrw pmpaddr0, {0}", in(reg) val),
            1 => core::arch::asm!("csrw pmpaddr1, {0}", in(reg) val),
            2 => core::arch::asm!("csrw pmpaddr2, {0}", in(reg) val),
            3 => core::arch::asm!("csrw pmpaddr3, {0}", in(reg) val),
            4 => core::arch::asm!("csrw pmpaddr4, {0}", in(reg) val),
            5 => core::arch::asm!("csrw pmpaddr5, {0}", in(reg) val),
            6 => core::arch::asm!("csrw pmpaddr6, {0}", in(reg) val),
            7 => core::arch::asm!("csrw pmpaddr7, {0}", in(reg) val),
            8 => core::arch::asm!("csrw pmpaddr8, {0}", in(reg) val),
            9 => core::arch::asm!("csrw pmpaddr9, {0}", in(reg) val),
            10 => core::arch::asm!("csrw pmpaddr10, {0}", in(reg) val),
            11 => core::arch::asm!("csrw pmpaddr11, {0}", in(reg) val),
            12 => core::arch::asm!("csrw pmpaddr12, {0}", in(reg) val),
            13 => core::arch::asm!("csrw pmpaddr13, {0}", in(reg) val),
            14 => core::arch::asm!("csrw pmpaddr14, {0}", in(reg) val),
            15 => core::arch::asm!("csrw pmpaddr15, {0}", in(reg) val),
            _ => unreachable!(),
        }
    }
}

// TODO: adapt this for 32bit
// According to the spec, RV64 has only even numbers for pmpcfgX. pmpcfg0, pmpcfg2,
// pmpcfg4...pmpcfg14
fn write_pmpcfg(index: usize, val: usize) {
    let n = index / 8;
    let shift = (index % 8) * 8;
    let old: usize;

    unsafe {
        match n {
            0 => core::arch::asm!("csrr {0}, pmpcfg0", out(reg) old),
            2 => core::arch::asm!("csrr {0}, pmpcfg2", out(reg) old),
            4 => core::arch::asm!("csrr {0}, pmpcfg4", out(reg) old),
            8 => core::arch::asm!("csrr {0}, pmpcfg8", out(reg) old),
            10 => core::arch::asm!("csrr {0}, pmpcfg10", out(reg) old),
            12 => core::arch::asm!("csrr {0}, pmpcfg12", out(reg) old),
            14 => core::arch::asm!("csrr {0}, pmpcfg14", out(reg) old),
            _ => unreachable!(),
        };
    }

    let mask = !(0xFF << shift);
    let new = (old & mask) | (val << shift);

    unsafe {
        match n {
            0 => core::arch::asm!("csrw pmpcfg0, {0}", in(reg) new),
            2 => core::arch::asm!("csrw pmpcfg2, {0}", in(reg) new),
            4 => core::arch::asm!("csrw pmpcfg4, {0}", in(reg) new),
            8 => core::arch::asm!("csrw pmpcfg8, {0}", in(reg) new),
            10 => core::arch::asm!("csrw pmpcfg10, {0}", in(reg)new),
            12 => core::arch::asm!("csrw pmpcfg12, {0}", in(reg) new),
            14 => core::arch::asm!("csrw pmpcfg14, {0}", in(reg) new),
            _ => unreachable!(),
        };
    }
}
