#[cfg(feature = "standalone")]
use common::sbi::SBI_SYSTEM_RESET_REASON_NO_REASON;
use common::sbi::{
    sbi_call, SbiRet, COVG_EXTENSION, COVG_GET_EVIDENCE, PAGE_SIZE, SBI_COVH_EXT_ID,
    SBI_COVH_RUN_TVM_VCPU, SBI_EXT_HSM, SBI_HSM_HART_SUSPEND, SBI_SYSTEM_RESET_EXT_ID,
    SBI_SYSTEM_RESET_TYPE_SHUTDOWN,
};
use riscv::interrupt::Trap;

use crate::{
    gpt::{map_4k_leaf, read_guest_memory, write_guest_memory, PTE_A, PTE_D, PTE_R, PTE_W, PTE_X},
    h_extension::{csrs::htval, instruction::hfence_gvma_all, HvException},
    println,
};

#[repr(C)]
#[derive(Clone, Debug)]
pub struct VmTrapContext {
    // Guest registers x0-x31 (Offset 0-248)
    // We save x0 as a placeholder to keep indexing simple: regs[i] == x(i)
    pub regs: [usize; 32],
    // Hypervisor Stack Pointer (Offset 256)
    pub hs_sp: usize,

    pub sepc: usize,
    pub sstatus: usize,
}

#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn hyper_trap() -> ! {
    core::arch::naked_asm!(
        // --- 1. ENTRY: Save Guest Context ---
        // Swap Guest t6 (x31) with sscratch (which holds pointer to VmTrapContext)
        "csrrw t6, sscratch, t6
        // Save Guest GPRs x1-x30 into the context
        sd x1,   8(t6)  // ra
        sd x2,  16(t6)  // sp
        sd x3,  24(t6)  // gp
        sd x4,  32(t6)  // tp
        sd x5,  40(t6)  // t0
        sd x6,  48(t6)  // t1
        sd x7,  56(t6)  // t2
        sd x8,  64(t6)  // s0
        sd x9,  72(t6)  // s1
        sd x10, 80(t6)  // a0
        sd x11, 88(t6)  // a1
        sd x12, 96(t6)  // a2
        sd x13, 104(t6) // a3
        sd x14, 112(t6) // a4
        sd x15, 120(t6) // a5
        sd x16, 128(t6) // a6
        sd x17, 136(t6) // a7
        sd x18, 144(t6) // s2
        sd x19, 152(t6) // s3
        sd x20, 160(t6) // s4
        sd x21, 168(t6) // s5
        sd x22, 176(t6) // s6
        sd x23, 184(t6) // s7
        sd x24, 192(t6) // s8
        sd x25, 200(t6) // s9
        sd x26, 208(t6) // s10
        sd x27, 216(t6) // s11
        sd x28, 224(t6) // t3
        sd x29, 232(t6) // t4
        sd x30, 240(t6) // t5
        csrr t0, sepc
        sd t0, 264(t6)
        csrr t0, sstatus
        sd t0, 272(t6)
        // Save the Guest's original t6 (currently in sscratch)
        csrr t0, sscratch
        sd t0, 248(t6)
        // --- 2. TRANSITION: Switch to HS-mode Stack ---
        ld sp, 256(t6) // Load hs_sp
        // Call the Rust handler.
        // a0 must be the pointer to VmTrapContext.
        mv a0, t6
        call hyper_trap_handler_rust
        // --- 3. EXIT: Restore Guest Context ---
        // Rust returns the pointer to VmTrapContext in a0
        mv t6, a0
        // Restore GPRs x1-x30, skip x5 (t0) because it is needed to do some stuff
        ld x1,   8(t6)
        ld x2,  16(t6)
        ld x3,  24(t6)
        ld x4,  32(t6)
        ld x6,  48(t6)
        ld x7,  56(t6)
        ld x8,  64(t6)
        ld x9,  72(t6)
        ld x10, 80(t6)
        ld x11, 88(t6)
        ld x12, 96(t6)
        ld x13, 104(t6)
        ld x14, 112(t6)
        ld x15, 120(t6)
        ld x16, 128(t6)
        ld x17, 136(t6)
        ld x18, 144(t6)
        ld x19, 152(t6)
        ld x20, 160(t6)
        ld x21, 168(t6)
        ld x22, 176(t6)
        ld x23, 184(t6)
        ld x24, 192(t6)
        ld x25, 200(t6)
        ld x26, 208(t6)
        ld x27, 216(t6)
        ld x28, 224(t6)
        ld x29, 232(t6)
        ld x30, 240(t6)
        /* Restore s* register using t0 register*/
        ld t0, 264(t6)
        csrw sepc, t0
        ld t0, 272(t6)
        csrw sstatus, t0
        /* Restore t0*/
        ld x5, 40(t6)
        csrw sscratch, t6 // Put VmTrapContext pointer back into sscratch
        ld t6, 248(t6)    // Finally restore Guest t6
        sret"
    )
}

#[no_mangle]
extern "C" fn hyper_trap_handler_rust(ctx: *mut VmTrapContext) -> *mut VmTrapContext {
    let scause = riscv::register::scause::read();
    let htval = htval::read();
    let stval = riscv::register::stval::read();
    let sepc = riscv::register::sepc::read();

    match scause.cause() {
        Trap::Interrupt(interrupt_number) => {
            panic!("Interrupt {} not handled", interrupt_number);
        }

        Trap::Exception(exception_number) => match exception_number {
            _ => match HvException::from(scause.code()) {
                HvException::EcallFromVsMode => {
                    let regs = unsafe { &mut (*ctx).regs };

                    let eid = regs[17];
                    let fid = regs[16];
                    let args = [regs[10], regs[11], regs[12], regs[13], regs[14], regs[15]];

                    let sbi_ret = match eid {
                        COVG_EXTENSION => handle_covg(fid, &args),
                        SBI_EXT_HSM => handle_guest_hsm(ctx, fid, &args),
                        SBI_SYSTEM_RESET_EXT_ID => handle_guest_system_reset(ctx),
                        /* fall back to opensbi */
                        _ => sbi_call(eid, fid, &args),
                    };

                    /* Return value and increment sepc */
                    unsafe {
                        (*ctx).regs[10] = sbi_ret.a0 as usize;
                        (*ctx).regs[11] = sbi_ret.a1 as usize;
                        (*ctx).sepc += 4;
                    }
                }

                HvException::InstructionGuestPageFault
                | HvException::LoadGuestPageFault
                | HvException::StoreAmoGuestPageFault => {
                    // 'stval' holds the Guest Physical Address that caused the fault
                    handle_page_fault(htval.bits(), stval);
                    // We do NOT increment sepc; we want to retry the instruction
                }
                _ => {
                    panic!(
                        "Unhandled Exception: {:?}, SEPC: {:#x}",
                        scause.cause(),
                        sepc
                    );
                }
            },
        },
    }

    ctx
}

fn handle_guest_system_reset(ctx: *mut VmTrapContext) -> SbiRet {
    let regs = unsafe { &mut (*ctx).regs };
    let reset_type = regs[10];
    match reset_type {
        SBI_SYSTEM_RESET_TYPE_SHUTDOWN => guest_shutdown_to_host(),
        _ => SbiRet { a0: -1, a1: 0 },
    }
}

fn handle_guest_hsm(ctx: *mut VmTrapContext, fid: usize, args: &[usize; 6]) -> SbiRet {
    match fid {
        SBI_HSM_HART_SUSPEND => guest_suspend(ctx),
        _ => sbi_call(SBI_EXT_HSM, fid, args),
    }
}

fn guest_shutdown_to_host() -> ! {
    let _owner = {
        let mut state_lock = crate::STATE.lock();
        let state = state_lock.as_mut().expect("TSM state not initialized");
        let tvmid = state
            .hypervisor
            .current_vcpu()
            .expect("trap with no running TVM")
            .tvmid;
        state
            .hypervisor
            .tvm_shutdown(tvmid)
            .expect("TVM is not running")
    };

    #[cfg(feature = "standalone")]
    {
        // Standalone TSM has no CoVE host to receive the stopped-TVM return.
        let _ = sbi_call(
            SBI_SYSTEM_RESET_EXT_ID,
            0,
            &[
                SBI_SYSTEM_RESET_TYPE_SHUTDOWN,
                SBI_SYSTEM_RESET_REASON_NO_REASON,
                0,
                0,
                0,
                0,
            ],
        );
        loop {}
    }

    #[cfg(not(feature = "standalone"))]
    return_to_host(_owner)
}

fn hyper_exit_to_host(state: &mut crate::TsmState, tvmid: usize) -> ! {
    let owner = state
        .hypervisor
        .tvm_shutdown(tvmid)
        .expect("TVM is not running");
    return_to_host(owner)
}

fn guest_suspend(ctx: *mut VmTrapContext) -> ! {
    unsafe {
        (*ctx).sepc += 4;
    }

    let mut state_lock = crate::STATE.lock();
    let state = state_lock.as_mut().expect("TSM state not initialized");
    let tvmid = state
        .hypervisor
        .current_vcpu()
        .expect("suspend with no running TVM")
        .tvmid;
    let owner = state
        .hypervisor
        .suspend_tvm(tvmid)
        .expect("failed to suspend TVM");
    drop(state_lock);

    return_to_host(owner)
}

fn return_to_host(owner: usize) -> ! {
    let return_fid = ((owner & 0x3F) << 26) | SBI_COVH_RUN_TVM_VCPU;

    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") 0usize,
            in("a1") 0usize,
            in("a6") return_fid,
            in("a7") SBI_COVH_EXT_ID,
            options(noreturn),
        );
    }
}

fn copy_page_data(
    pa: usize,
    gpa_page: usize,
    gpa_page_end: usize,
    source_gpa: usize,
    source_offset: usize,
    source: &[u8],
) {
    let source_end = source_gpa + source.len();
    if gpa_page >= source_end || source_gpa >= gpa_page_end {
        return;
    }

    let copy_gpa_start = core::cmp::max(gpa_page, source_gpa);
    let copy_gpa_end = core::cmp::min(gpa_page_end, source_end);
    let source_offset = source_offset + copy_gpa_start - source_gpa;
    let destination_offset = copy_gpa_start - gpa_page;
    let copy_length = copy_gpa_end - copy_gpa_start;

    unsafe {
        core::ptr::copy_nonoverlapping(
            source.as_ptr().add(source_offset),
            (pa as *mut u8).add(destination_offset),
            copy_length,
        );
    }
}

fn handle_page_fault(htval: usize, stval: usize) {
    let gpa = (htval << 2) | (stval & 0x3);
    let gpa_page = gpa & !(PAGE_SIZE - 1);
    let gpa_page_end = gpa_page + PAGE_SIZE;

    let mut state_lock = crate::STATE.lock();
    let state = state_lock.as_mut().expect("TSM state not initialized");
    let tvmid = state
        .hypervisor
        .current_vcpu()
        .expect("page fault with no running TVM")
        .tvmid;

    let in_tvm_region = state
        .hypervisor
        .tvm(tvmid)
        .is_some_and(|tvm| tvm.contains_gpa_range(gpa_page, gpa_page_end));
    if !in_tvm_region {
        println!(
            "[OLORIN] Guest page fault outside TVM memory regions: GPA=0x{:x}",
            gpa
        );
        hyper_exit_to_host(state, tvmid);
    }

    let (page_table_addr, page_table_size) = state
        .hypervisor
        .tvm(tvmid)
        .map(|tvm| tvm.page_table())
        .expect("running TVM disappeared");

    let lazy = match state.hypervisor.running_tvm_mut() {
        Some(tvm) => match tvm.lazy_state_mut() {
            Some(lazy) => lazy,
            None => {
                println!(
                    "[OLORIN] Guest page fault with no lazy state: GPA=0x{:x}",
                    gpa
                );
                hyper_exit_to_host(state, tvmid);
            }
        },
        None => {
            println!("[OLORIN] Page fault with no running TVM: GPA=0x{:x}", gpa);
            hyper_exit_to_host(state, tvmid);
        }
    };
    // Allocate a physical page (simple bump allocation)
    if lazy.next_free_phys >= lazy.phys_limit {
        panic!(
            "OOM: run out of confidential memory for lazy loading (gpa=0x{:x})",
            gpa,
        );
    }

    let pa = lazy.next_free_phys;
    lazy.next_free_phys += PAGE_SIZE;

    // println!(
    //     "[OLORIN] page_fault_handler: htval 0x{:x}; stval 0x{:x}; gpa 0x{:x}; gpa_page 0x{:x}; next_free_phys 0x{:x}; phys_limit 0x{:x}",
    //     htval, stval, gpa, gpa_page, lazy.next_free_phys, lazy.phys_limit
    // );

    // Initialize page with zeros (important for BSS or partial pages)
    unsafe { core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE) };

    copy_page_data(pa, gpa_page, gpa_page_end, lazy.dtb_gpa, 0, lazy.dtb_data);
    copy_page_data(
        pa,
        gpa_page,
        gpa_page_end,
        lazy.initrd_gpa,
        0,
        lazy.initrd_data,
    );

    // Fill with ELF data if the page overlaps a segment
    for segment in &lazy.segments {
        let seg_start = segment.gpa;
        let seg_end = segment.gpa + segment.memsz;

        if gpa_page_end <= seg_start || gpa_page >= seg_end {
            continue;
        }

        copy_page_data(
            pa,
            gpa_page,
            gpa_page_end,
            segment.gpa,
            segment.offset,
            lazy.elf_data,
        );
    }

    // Map the page into the Guest Page Table
    // Retrieve the root PPN from HGATP to find the page table location
    // Map with full permissions for now (R/W/X/U)
    map_4k_leaf(
        page_table_addr,
        page_table_size,
        gpa_page,
        pa,
        PTE_R | PTE_W | PTE_X | PTE_A | PTE_D,
    );

    // 6. Flush TLB so the CPU sees the new mapping immediately
    hfence_gvma_all();
}

fn handle_covg(fid: usize, args: &[usize; 6]) -> SbiRet {
    match fid {
        COVG_GET_EVIDENCE => {
            println!("[OLORIN] Requested attestation certificate");
            handle_covg_get_evidence(args[0], args[1], args[2], args[3], args[4], args[5])
        }
        _ => SbiRet { a0: -1, a1: 0 },
    }
}

fn handle_covg_get_evidence(
    _pub_key_addr: usize,
    _pub_key_size: usize,
    challenge_addr: usize,
    cert_format: usize,
    cert_addr_out: usize,
    cert_size: usize,
) -> SbiRet {
    // The existing guest ABI uses zero for the CBOR format. Keep that
    // compatibility behavior and reject all other formats.
    if cert_format != 0 {
        return SbiRet { a0: -1, a1: 0 };
    }

    let hgatp_val = crate::h_extension::csrs::hgatp::read().bits();
    let root_pt = ((hgatp_val & 0xFF_FFFF_FFFF_F) << 12) as usize;

    let mut challenge = [0u8; 64];
    if read_guest_memory(root_pt, challenge_addr, &mut challenge).is_err() {
        return SbiRet { a0: -1, a1: 0 }; // Fault or Boundary Error
    }

    let encoded_evidence = {
        let state_lock = crate::STATE.lock();
        let state = match state_lock.as_ref() {
            Some(state) => state,
            None => return SbiRet { a0: -1, a1: 0 },
        };

        let tvm = match state.hypervisor.running_tvm() {
            Some(tvm) => tvm,
            None => return SbiRet { a0: -1, a1: 0 },
        };

        let evidence = tvm.get_evidence(&challenge);
        match evidence.to_bytes() {
            Ok(encoded) => encoded,
            Err(error) => {
                println!("[OLORIN] Error during evidence encoding {}", error);
                return SbiRet { a0: -1, a1: 0 };
            }
        }
    };

    if encoded_evidence.len() > cert_size {
        return SbiRet { a0: -1, a1: 0 }; // Buffer too small
    }

    if write_guest_memory(root_pt, cert_addr_out, &encoded_evidence).is_err() {
        return SbiRet { a0: -1, a1: 0 };
    }

    // Success
    SbiRet {
        a0: 0,
        a1: encoded_evidence.len() as isize,
    }
}
