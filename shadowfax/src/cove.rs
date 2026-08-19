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
    COVH_DEFAULT_PAGE_SIZE, PAGE_SIZE, SBI_COVH_CONVERT_PAGES, SBI_COVH_CREATE_TVM,
    SBI_COVH_EXT_ID, SBI_COVH_FINALIZE_TVM, SBI_COVH_GET_TSM_INFO, SBI_COVH_RECLAIM_PAGES,
    SBI_EXT_SUPD_GET_ACTIVE_DOMAINS, SBI_SUPD_EXT_ID,
};
use heapless::Vec;
use zeroize::Zeroize;

use crate::{
    _tee_stack_top,
    context::Context,
    domain::MemoryRegion,
    opensbi,
    platform::MAX_MEMORY_REGIONS,
    state::{Allocation, BorrowKind, State, STATE},
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
    let src_id = unsafe {
        let hart_id = riscv::register::mhartid::read();
        let hart_index = opensbi::sbi_hartid_to_hartindex(hart_id as u32);
        let domain = opensbi::sbi_hartindex_to_domain(hart_index);
        (*domain).index as usize
    };

    let (dst_id, fid) = cove_unpack_fid!(fid);

    // Scratch space
    let scratch_start = &raw const _tee_stack_top as *const u8 as usize;
    let base_ctx = scratch_start - (TEE_SCRATCH_SIZE + size_of::<Context>());
    let scratch_ctx = base_ctx as *mut Context;

    if dst_id >= state.domains.len() {
        return unsafe { return_error(base_ctx, -1) };
    }

    // TEECALL
    if state.domains[dst_id].has_tsm {
        let domain_ctx = state.domains[dst_id].context_addr as *mut Context;
        // check if the domain is trusted. If not just return an error to the caller
        if !state.domains[dst_id].is_trusted(src_id) {
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

            // save the caller context address into domain context
            (*domain_ctx).caller_ctx = caller_ctx_addr;
        }

        // Perform operations to allow the specific functionality
        match fid {
            SBI_COVH_CREATE_TVM => {
                state.start_bootstrap(src_id, dst_id).unwrap();
            }
            // For sbi_covh_get_domain_info we need to give the TSM access to the memory space
            // where he will write the domain_info struct (a0) for the necessary size (a1).
            SBI_COVH_GET_TSM_INFO => {
                let base_address = unsafe { (*domain_ctx).regs[10] };
                let size = unsafe { (*domain_ctx).regs[11] };

                // Base address must be page aligned, we cannot exceed number of available pmp
                // registers
                assert!(base_address % COVH_DEFAULT_PAGE_SIZE == 0);

                let order = if (size & (size - 1)) == 0 {
                    size.trailing_zeros()
                } else {
                    size.next_power_of_two().trailing_zeros()
                }
                .max(3);

                /*
                 * Borrow the domain only for this operation.
                 */
                {
                    let domain = &mut state.domains[dst_id];

                    domain
                        .memory_regions
                        .push(MemoryRegion {
                            base_address,
                            order,
                            mmio: false,
                            permissions: 0x3f,
                        })
                        .unwrap();
                }
            }
            SBI_COVH_CONVERT_PAGES => {
                let base_address = unsafe { (*domain_ctx).regs[10] };
                let num_pages = unsafe { (*domain_ctx).regs[11] };

                // Base address must be page aligned, we cannot exceed number of available pmp
                // registers
                assert!(base_address % COVH_DEFAULT_PAGE_SIZE == 0);

                let ticket = state
                    .request_borrow(BorrowKind::new_alloc(
                        src_id,
                        dst_id,
                        base_address,
                        num_pages,
                    ))
                    .unwrap();

                /* Store the ticket in a6 register */
                unsafe {
                    (*domain_ctx).regs[16] |= (ticket & 0x3FF) << 16;
                }
            }

            SBI_COVH_RECLAIM_PAGES => {
                let base_address = unsafe { (*domain_ctx).regs[10] };
                let num_pages = unsafe { (*domain_ctx).regs[11] };

                let ticket = state
                    .request_borrow(BorrowKind::new_reclaim(
                        src_id,
                        dst_id,
                        base_address,
                        num_pages,
                    ))
                    .unwrap();

                /* Store the ticket in a6 register */
                unsafe {
                    (*domain_ctx).regs[16] |= (ticket & 0x3FF) << 16;
                }
            }
            _ => {}
        }
        unsafe {
            let ret = opensbi::sbi_domain_change_active(dst_id as u32);
            assert!(ret == 0);
        }
    } else {
        // TEERET
        // We don't need to store the calling context since we are implementing the
        // non reentrant TSM. We need a0 and a1 registers to deliver the result
        //
        // dst_id is the untrusted domain

        // Identify the TSM which accepted this caller according to the DT trust map.

        let success = unsafe {
            let domain_ctx = state.domains[dst_id].context_addr as *mut Context;
            let eid = (*scratch_ctx).regs[16] & 0xFFFF;
            (*domain_ctx).regs[10] = (*scratch_ctx).regs[10];
            (*domain_ctx).regs[11] = (*scratch_ctx).regs[11];
            (*domain_ctx).regs[16] = (src_id << 26) | eid;
            /* increment mepc to avoid loop */
            (*domain_ctx).mepc += 4;

            /* Based on a0 register we know if the TEECALL was successful */
            (*domain_ctx).regs[10] as isize == 0
        };

        match fid {
            SBI_COVH_CONVERT_PAGES => {
                /* Confirm the borrow */
                let a6 = unsafe { (*scratch_ctx).regs[16] };
                let ticket = (a6 & (0x3FF << 16)) >> 16;
                if success {
                    let Allocation {
                        base_address,
                        num_pages,
                        owner_id,
                        tsm_id,
                    } = state.take_borrow(ticket).unwrap();
                    let order = (num_pages * COVH_DEFAULT_PAGE_SIZE).trailing_zeros();
                    /* Remove the region from the dst domain (aka the untrusted) */
                    let domain = &mut state.domains[dst_id];

                    domain.memory_regions = compute_new_regions(
                        &domain.memory_regions,
                        base_address,
                        base_address + num_pages * PAGE_SIZE,
                    )
                    .unwrap();

                    /* Add the region to the src domain (aka the tsm)*/
                    let tsm = &mut state.domains[src_id];
                    tsm.memory_regions
                        .push(MemoryRegion {
                            base_address,
                            order,
                            mmio: false,
                            permissions: 0x3f,
                        })
                        .unwrap();
                    /* Zero out the memory region */
                    {
                        let vec = unsafe {
                            core::slice::from_raw_parts_mut(
                                base_address as *mut u8,
                                num_pages * PAGE_SIZE,
                            )
                        };
                        vec.zeroize();
                    }
                } else {
                    state.cancel_borrow(ticket).unwrap();
                }
            }
            SBI_COVH_RECLAIM_PAGES => {
                /* Confirm the borrow */
                let a6 = unsafe { (*scratch_ctx).regs[16] };
                let ticket = (a6 & (0x3FF << 16)) >> 16;
                if success {
                    let Allocation {
                        base_address,
                        num_pages,
                        owner_id,
                        tsm_id,
                    } = state.take_borrow(ticket).unwrap();
                    let order = (num_pages * COVH_DEFAULT_PAGE_SIZE).trailing_zeros();
                    /* Remove the region from the src domain (aka the tsm ) */
                    let tsm = &mut state.domains[src_id];

                    tsm.memory_regions = compute_new_regions(
                        &tsm.memory_regions,
                        base_address,
                        base_address + num_pages * PAGE_SIZE,
                    )
                    .unwrap();

                    /* Add the region to the dst (aka the untrusted domain)*/
                    let tsm = &mut state.domains[dst_id];
                    tsm.memory_regions
                        .push(MemoryRegion {
                            base_address,
                            order,
                            mmio: false,
                            permissions: 0x3f,
                        })
                        .unwrap();
                    /* Zero out the memory block */
                    {
                        let vec = unsafe {
                            core::slice::from_raw_parts_mut(
                                base_address as *mut u8,
                                num_pages * PAGE_SIZE,
                            )
                        };
                        vec.zeroize();
                    }
                } else {
                    state.cancel_borrow(ticket).unwrap();
                }
            }
            SBI_COVH_FINALIZE_TVM => {
                if success {
                    state.finish_bootstrap(dst_id, src_id).unwrap();
                }
            }
            _ => {}
        }
        unsafe {
            let ret = opensbi::sbi_domain_change_active(dst_id as u32);
            assert!(ret == 0);
        }
    }

    program_domain_pmp(state, dst_id);
    return state.domains[dst_id].context_addr;
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
fn program_domain_pmp(state: &State, domain_id: usize) {
    let mut regions = Vec::<MemoryRegion, MAX_MEMORY_REGIONS>::new();

    for region in &state.domains[domain_id].memory_regions {
        regions.push(region.clone()).unwrap();
    }

    if let Some(owner_id) = state.bootstrap_owner_for(domain_id) {
        for region in &state.domains[owner_id].memory_regions {
            regions
                .push(region.clone())
                .expect("bootstrap PMP map exceeds available entries");
        }
    }

    program_pmp_from_regions(&regions);
}

fn program_pmp_from_regions(regions: &[MemoryRegion]) {
    // Disable all managed entries first: no stale bootstrap access.
    for index in 0..MAX_MEMORY_REGIONS {
        write_pmpcfg(index, 0);
    }
    for (i, r) in regions.iter().enumerate() {
        let ones = (1 << (r.order - 3)) - 1;
        let range = riscv::register::Range::NAPOT as usize;
        let permission = (r.permissions & 0x7) as usize;

        // This should be a byte and be shifted by index
        let pmpcfg = ((0) << 7 | (range) << 3 | (permission)) & 0xFF;
        let pmpaddr = ((r.base_address >> 2) as usize) | ones as usize;

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

// TODO: adapt this for 32bit.
// On RV64 each implemented pmpcfg CSR holds eight entries: pmpcfg0 controls
// entries 0..7 and pmpcfg2 controls entries 8..15.
fn write_pmpcfg(index: usize, val: usize) {
    assert!(index < 16);

    let cfg_index = index / 8;
    let shift = (index % 8) * 8;
    let old: usize;

    unsafe {
        match cfg_index {
            0 => core::arch::asm!("csrr {0}, pmpcfg0", out(reg) old),
            1 => core::arch::asm!("csrr {0}, pmpcfg2", out(reg) old),
            _ => unreachable!(),
        };
    }

    let mask = !(0xFF << shift);
    let new = (old & mask) | (val << shift);

    unsafe {
        match cfg_index {
            0 => core::arch::asm!("csrw pmpcfg0, {0}", in(reg) new),
            1 => core::arch::asm!("csrw pmpcfg2, {0}", in(reg) new),
            _ => unreachable!(),
        };
    }
}

/* Remove the given memory range from Domain region list. It addresses overlapping before and after.
 * If a region contains the target start and target end it will be split into 2 regions
 * */
fn compute_new_regions(
    regions: &Vec<MemoryRegion, MAX_MEMORY_REGIONS>,
    target_start: usize,
    target_end: usize,
) -> anyhow::Result<Vec<MemoryRegion, MAX_MEMORY_REGIONS>> {
    let mut new_regions = Vec::new();

    for region in regions {
        let region_start = region.base_address;
        let region_end = region_start + (1 << region.order);

        // Case 1: No overlap - keep the region as is
        if region_end <= target_start || region_start >= target_end {
            new_regions
                .push(region.clone())
                .map_err(|_| anyhow::anyhow!("cannot push memory region"))?;
            continue;
        }

        // Case 2: Keep the fragment before the removed range
        if region_start < target_start {
            let before_end = target_start.min(region_end);

            add_region_range(
                &mut new_regions,
                region_start,
                before_end,
                region.mmio,
                region.permissions,
            )?;
        }
        // Case 3: Keep the fragment after the removed range
        if region_end > target_end {
            let after_start = target_end.max(region_start);
            add_region_range(
                &mut new_regions,
                after_start,
                region_end,
                region.mmio,
                region.permissions,
            )?;
        }
    }
    Ok(new_regions)
}

fn add_region_range(
    regions: &mut Vec<MemoryRegion, MAX_MEMORY_REGIONS>,
    mut start: usize,
    end: usize,
    mmio: bool,
    permissions: u8,
) -> anyhow::Result<()> {
    while start < end {
        let remaining = end - start;

        // Largest block allowed by the start address alignment.
        let alignment_order = start.trailing_zeros() as usize;

        // Largest power-of-two that fits in the remaining range.
        let size_order = (usize::BITS - 1 - remaining.leading_zeros()) as usize;

        let order = alignment_order.min(size_order);

        regions
            .push(MemoryRegion {
                base_address: start,
                order: order as u32,
                mmio,
                permissions,
            })
            .map_err(|_| anyhow::anyhow!("cannot push memory region"))?;

        start += 1usize << order;
    }

    Ok(())
}
