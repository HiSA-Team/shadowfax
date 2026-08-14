use alloc::vec::Vec;
use common::{
    attestation::DiceLayer,
    sbi::{
        sbi_call, SbiRet, COVG_EXTENSION, COVG_GET_EVIDENCE, PAGE_SIZE, SBI_COVH_EXT_ID,
        SBI_SYSTEM_RESET_EXT_ID, SBI_SYSTEM_RESET_TYPE_SHUTDOWN,
    },
};
use riscv::interrupt::Trap;
use spin::Mutex;

use crate::{
    constants::{
        GUEST_DRAM_GPA_END, GUEST_DRAM_GPA_START, PTE_A, PTE_D, PTE_R, PTE_U, PTE_W, PTE_X,
    },
    h_extension::{
        csrs::{hgatp, htval},
        instruction::hfence_gvma_all,
        HvException,
    },
    hyper::{map_4k_leaf, read_guest_memory, write_guest_memory},
    println, ATTESTATION_CONTEXT, MEASUREMENT,
};

#[repr(C)]
#[derive(Clone, Debug)]
pub struct VmTrapContext {
    // Guest registers x0-x31 (Offset 0-248)
    // We save x0 as a placeholder to keep indexing simple: regs[i] == x(i)
    pub regs: [usize; 32],
    // Hypervisor Stack Pointer (Offset 256)
    pub hs_sp: usize,
}

// Track ELF segments to know what to copy where
pub struct LazySegment {
    gpa: usize,
    memsz: usize,
    filesz: usize,
    offset: usize,
}

impl LazySegment {
    pub fn new(gpa: usize, memsz: usize, filesz: usize, offset: usize) -> Self {
        Self {
            gpa,
            memsz,
            filesz,
            offset,
        }
    }
}

// Global state accessible by the trap handler
pub struct LazyState {
    // elf
    segments: Vec<LazySegment>,
    elf_data: &'static [u8],

    // dtb
    dtb_gpa: usize,
    dtb_data: &'static [u8],

    // initrd
    initrd_gpa: usize,
    initrd_data: &'static [u8],

    // allocator
    next_free_phys: usize, // Simple bump allocator for physical pages
    phys_limit: usize,
    page_table_size: usize,
}

impl LazyState {
    pub fn new(
        segments: Vec<LazySegment>,
        elf_data: &'static [u8],
        dtb_gpa: usize,
        dtb_data: &'static [u8],
        initrd_gpa: usize,
        initrd_data: &'static [u8],
        next_free_phys: usize,
        phys_limit: usize,
        page_table_size: usize,
    ) -> Self {
        Self {
            segments,
            elf_data,
            dtb_gpa,
            dtb_data,
            initrd_gpa,
            initrd_data,
            next_free_phys,
            phys_limit,
            page_table_size,
        }
    }
}

// Mutex to safely access this from the trap handler
pub static LAZY_STATE: Mutex<Option<LazyState>> = Mutex::new(None);

#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn hyper_trap() -> ! {
    core::arch::naked_asm!(
        // --- 1. ENTRY: Save Guest Context ---
        // Swap Guest t6 (x31) with sscratch (which holds pointer to VmTrapContext)
        "csrrw t6, sscratch, t6",
        // Save Guest GPRs x1-x30 into the context
        "sd x1,   8(t6)",  // ra
        "sd x2,  16(t6)",  // sp
        "sd x3,  24(t6)",  // gp
        "sd x4,  32(t6)",  // tp
        "sd x5,  40(t6)",  // t0
        "sd x6,  48(t6)",  // t1
        "sd x7,  56(t6)",  // t2
        "sd x8,  64(t6)",  // s0
        "sd x9,  72(t6)",  // s1
        "sd x10, 80(t6)",  // a0
        "sd x11, 88(t6)",  // a1
        "sd x12, 96(t6)",  // a2
        "sd x13, 104(t6)", // a3
        "sd x14, 112(t6)", // a4
        "sd x15, 120(t6)", // a5
        "sd x16, 128(t6)", // a6
        "sd x17, 136(t6)", // a7
        "sd x18, 144(t6)", // s2
        "sd x19, 152(t6)", // s3
        "sd x20, 160(t6)", // s4
        "sd x21, 168(t6)", // s5
        "sd x22, 176(t6)", // s6
        "sd x23, 184(t6)", // s7
        "sd x24, 192(t6)", // s8
        "sd x25, 200(t6)", // s9
        "sd x26, 208(t6)", // s10
        "sd x27, 216(t6)", // s11
        "sd x28, 224(t6)", // t3
        "sd x29, 232(t6)", // t4
        "sd x30, 240(t6)", // t5
        // Save the Guest's original t6 (currently in sscratch)
        "csrr t0, sscratch",
        "sd t0, 248(t6)",
        // --- 2. TRANSITION: Switch to HS-mode Stack ---
        "ld sp, 256(t6)", // Load hs_sp
        // Call the Rust handler.
        // a0 must be the pointer to VmTrapContext.
        "mv a0, t6",
        "call hyper_trap_handler_rust",
        // --- 3. EXIT: Restore Guest Context ---
        // Rust returns the pointer to VmTrapContext in a0
        "mv t6, a0",
        // Restore GPRs x1-x30
        "ld x1,   8(t6)",
        "ld x2,  16(t6)",
        "ld x3,  24(t6)",
        "ld x4,  32(t6)",
        "ld x5,  40(t6)",
        "ld x6,  48(t6)",
        "ld x7,  56(t6)",
        "ld x8,  64(t6)",
        "ld x9,  72(t6)",
        "ld x10, 80(t6)",
        "ld x11, 88(t6)",
        "ld x12, 96(t6)",
        "ld x13, 104(t6)",
        "ld x14, 112(t6)",
        "ld x15, 120(t6)",
        "ld x16, 128(t6)",
        "ld x17, 136(t6)",
        "ld x18, 144(t6)",
        "ld x19, 152(t6)",
        "ld x20, 160(t6)",
        "ld x21, 168(t6)",
        "ld x22, 176(t6)",
        "ld x23, 184(t6)",
        "ld x24, 192(t6)",
        "ld x25, 200(t6)",
        "ld x26, 208(t6)",
        "ld x27, 216(t6)",
        "ld x28, 224(t6)",
        "ld x29, 232(t6)",
        "ld x30, 240(t6)",
        "csrw sscratch, t6", // Put VmTrapContext pointer back into sscratch
        "ld t6, 248(t6)",    // Finally restore Guest t6
        "sret",
    )
}

#[no_mangle]
extern "C" fn hyper_trap_handler_rust(ctx: *mut VmTrapContext) -> *mut VmTrapContext {
    let scause = riscv::register::scause::read();
    let htval = htval::read();
    let stval = riscv::register::stval::read();
    let mut sepc = riscv::register::sepc::read();

    match scause.cause() {
        Trap::Interrupt(interrupt_number) => {
            panic!("Interrupt {} not handled", interrupt_number);
        }

        Trap::Exception(exception_number) => match exception_number {
            _ => match HvException::from(scause.code()) {
                HvException::EcallFromVsMode => {
                    let regs = unsafe { &mut (*ctx).regs };

                    let eid = regs[17];

                    let sbi_ret = match eid {
                        COVG_EXTENSION => handle_covg(
                            regs[16],
                            &[regs[10], regs[11], regs[12], regs[13], regs[14], regs[15]],
                        ),
                        SBI_SYSTEM_RESET_EXT_ID => handle_guest_system_reset(ctx),
                        /* fall back to opensbi */
                        _ => sbi_call(
                            regs[17],
                            regs[16],
                            &[regs[10], regs[11], regs[12], regs[13], regs[14], regs[15]],
                        ),
                    };

                    // 1.Check if the call was a CoVE-G
                    // 3. Write return values back to Guest a0, a1
                    regs[10] = sbi_ret.a0 as usize;
                    regs[11] = sbi_ret.a1 as usize;

                    // 3. Skip the 'ecall' instruction in the guest
                    sepc += 4;

                    unsafe {
                        riscv::register::sepc::write(sepc);
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

fn guest_shutdown_to_host() -> ! {
    let return_fid = {
        let mut state = crate::STATE.lock();
        state.as_mut().unwrap().hypervisor.guest_shutdown().unwrap()
    };

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

fn handle_page_fault(htval: usize, stval: usize) {
    // let cycle_start = read_cycle();
    let mut lock = LAZY_STATE.lock();
    let gpa = (htval << 2) | (stval & 0x3);
    let gpa_page = gpa & !(PAGE_SIZE - 1);
    let gpa_page_end = gpa_page + PAGE_SIZE;

    if gpa_page < GUEST_DRAM_GPA_START || gpa_page_end > GUEST_DRAM_GPA_END {
        panic!("GPA 0x{:x} is outside guest RAM", gpa);
    }

    if let Some(lazy) = lock.as_mut() {
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

        let dtb_start = lazy.dtb_gpa;
        let dtb_end = lazy.dtb_gpa + lazy.dtb_data.len();
        if gpa_page < dtb_end && dtb_start < gpa_page_end {
            let copy_gpa_start = core::cmp::max(gpa_page, dtb_start);
            let copy_gpa_end = core::cmp::min(gpa_page_end, dtb_end);

            let source_offset = copy_gpa_start - dtb_start;
            let destination_offset = copy_gpa_start - gpa_page;
            let copy_length = copy_gpa_end - copy_gpa_start;

            unsafe {
                core::ptr::copy_nonoverlapping(
                    lazy.dtb_data.as_ptr().add(source_offset),
                    (pa as *mut u8).add(destination_offset),
                    copy_length,
                );
            }
        }

        let initrd_start = lazy.initrd_gpa;
        let initrd_end = lazy.initrd_gpa + lazy.initrd_data.len();
        if gpa_page < initrd_end && initrd_start < gpa_page_end {
            let copy_gpa_start = core::cmp::max(gpa_page, initrd_start);
            let copy_gpa_end = core::cmp::min(gpa_page_end, initrd_end);

            let source_offset = copy_gpa_start - initrd_start;
            let destination_offset = copy_gpa_start - gpa_page;
            let copy_length = copy_gpa_end - copy_gpa_start;

            unsafe {
                core::ptr::copy_nonoverlapping(
                    lazy.initrd_data.as_ptr().add(source_offset),
                    (pa as *mut u8).add(destination_offset),
                    copy_length,
                );
            }
        }

        // Fill with ELF data if the page overlaps a segment
        for segment in &lazy.segments {
            let seg_start = segment.gpa;
            let seg_end = segment.gpa + segment.memsz;

            if gpa_page_end <= seg_start || gpa_page >= seg_end {
                continue;
            }

            let segment_file_start = segment.gpa;
            let segment_file_end = segment.gpa + segment.filesz;

            // Intersection of this page and the file-backed portion.
            let copy_gpa_start = core::cmp::max(gpa_page, segment_file_start);
            let copy_gpa_end = core::cmp::min(gpa_page_end, segment_file_end);

            if copy_gpa_start >= copy_gpa_end {
                // This is BSS: page was already zeroed.
                continue;
            }

            let source_offset = segment.offset + (copy_gpa_start - segment.gpa);
            let destination_offset = copy_gpa_start - gpa_page;
            let copy_length = copy_gpa_end - copy_gpa_start;

            // println!(
            //     "[OLORIN] page_fault_handler: copy ELF offset 0x{:x}, len {} -> GPA 0x{:x}, HPA 0x{:x}",
            //     source_offset,
            //     copy_length,
            //     copy_gpa_start,
            //     pa + destination_offset,
            // );

            unsafe {
                core::ptr::copy_nonoverlapping(
                    lazy.elf_data.as_ptr().add(source_offset),
                    (pa as *mut u8).add(destination_offset),
                    copy_length,
                );
            }
        }

        // Map the page into the Guest Page Table
        // Retrieve the root PPN from HGATP to find the page table location
        let hgatp_val = hgatp::read().bits();
        let root_ppn = hgatp_val & 0xFF_FFFF_FFFF_F;
        let root_pt = (root_ppn << 12) as usize;

        // Map with full permissions for now (R/W/X/U)
        map_4k_leaf(
            root_pt,
            lazy.page_table_size,
            gpa_page,
            pa,
            PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D,
        );

        // 6. Flush TLB so the CPU sees the new mapping immediately
        hfence_gvma_all();
    } else {
        panic!(
            "Guest Page Fault occurred but Lazy Loading state is not initialized! (GPA={:x}; gpa_page={:x})",
            gpa,gpa_page
        );
    }
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
    _cert_format: usize,
    cert_addr_out: usize,
    cert_size: usize,
) -> SbiRet {
    // A. SETUP: Get Page Table
    let hgatp_val = crate::h_extension::csrs::hgatp::read().bits();
    let root_pt = ((hgatp_val & 0xFF_FFFF_FFFF_F) << 12) as usize;

    // B. INPUT: Read Challenge from Guest
    let mut challenge = [0u8; 64];
    if read_guest_memory(root_pt, challenge_addr, &mut challenge).is_err() {
        return SbiRet { a0: -1, a1: 0 }; // Fault or Boundary Error
    }

    // C. LOGIC: Generate Evidence (Holds Locks)
    let encoded_evidence = {
        // We assume Measurement is also available here or passed in
        // For this example, let's say it's in TSM or separate lock
        let measure_lock = MEASUREMENT.lock();
        let measurement = match measure_lock.as_ref() {
            Some(m) => m,
            None => return SbiRet { a0: -1, a1: 0 },
        };

        let attetstation_lock = ATTESTATION_CONTEXT.lock();
        let tvm_attestation_ctx = match attetstation_lock.as_ref() {
            Some(att) => att,
            None => return SbiRet { a0: -1, a1: 0 },
        }
        .compute_next(measurement);
        let evidence = tvm_attestation_ctx.get_evidence(&measurement, &challenge);
        match evidence.to_bytes() {
            Ok(e) => e,
            Err(e) => {
                println!("[OLORIN] Error during evidence encoding {}", e);
                return SbiRet { a0: -1, a1: 0 };
            }
        }
    };
    // D. VALIDATION: Check Size
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
