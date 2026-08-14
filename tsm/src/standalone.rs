use core::alloc::Layout;

use common::{attestation::DiceLayer, sbi::PAGE_SIZE};
use elf::{abi::PT_LOAD, endian::AnyEndian, ElfBytes};

use crate::{
    constants::{GUEST_DRAM_GPA_START, GUEST_DRAM_SIZE, UART_GPA, UART_HPA},
    hyper::HypervisorState,
    println,
    trap::{LazySegment, LazyState, LAZY_STATE},
    TsmState, _secure_init, STATE,
};

#[link_section = ".rodata"]
static GUEST_ELF: [u8; include_bytes!(concat!(env!("OUT_DIR"), "/guest.elf")).len()] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/guest.elf"));

#[link_section = ".rodata"]
static GUEST_DTB: [u8; include_bytes!(concat!(env!("OUT_DIR"), "/guest.dtb")).len()] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/guest.dtb"));

#[link_section = ".rodata"]
static GUEST_INITRD: [u8; include_bytes!(concat!(env!("OUT_DIR"), "/guest.initrd")).len()] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/guest.initrd"));

/// Test function to bypass SBI and jump straight into a TVM
pub fn test_tvm_bootstrap() -> ! {
    println!("[OLORIN] Starting Mapping TVM from ELF");
    // We'll simulate a dummy attestation context for testing.
    _secure_init(0);

    let mut lock = STATE.lock();
    let state = lock.as_mut().expect("State not initialized");

    // Assuming TSM is at 0x90000000-0x93FFFFFFFF, put TVM structures higher up.
    //
    let confidential_memory = 0x94000000;
    let guest_ram_size = 256 * 1024 * 1024;
    let tvm_page_table_addr = confidential_memory;
    let tvm_state_addr = tvm_page_table_addr + 1024 * 1024;
    let tvm_confidential_pool = tvm_state_addr + 4096;
    let pool_size_pages = guest_ram_size / PAGE_SIZE;

    state
        .hypervisor
        .add_confidential_pages(tvm_page_table_addr, 256)
        .unwrap(); // 1 MiB
    state
        .hypervisor
        .add_confidential_pages(tvm_state_addr, 1)
        .unwrap();
    state
        .hypervisor
        .add_confidential_pages(tvm_confidential_pool, pool_size_pages)
        .unwrap();

    // 4. Use the ELF loading procedure
    // This helper parses GUEST_ELF and maps it into the TVM
    let tvm_id = bootstrap_load_elf_lazy(
        state,
        tvm_page_table_addr,
        tvm_state_addr,
        tvm_confidential_pool,
    )
    .expect("Failed to load ELF");

    // 5. Create VCPU (ID 0)
    state
        .hypervisor
        .create_tvm_vcpu(tvm_id, 0, 0)
        .expect("Failed to create VCPU");

    println!("[OLORIN] Bootstrap complete. Entering Guest...");

    // 6. Run it!
    let (vcpu_addr, entry_sepc, entry_arg) = state
        .hypervisor
        .prepare_tvm_vcpu(tvm_id, 0)
        .expect("Failed to prepare VCPU");
    drop(lock);

    unsafe { HypervisorState::enter_prepared_tvm_vcpu(vcpu_addr, entry_sepc, entry_arg) }
}

fn bootstrap_load_elf_lazy(
    state: &mut TsmState,
    pt_addr: usize,
    state_addr: usize,
    conf_pool_base: usize,
) -> anyhow::Result<usize> {
    let elf = unsafe {
        ElfBytes::<AnyEndian>::minimal_parse(core::slice::from_raw_parts(
            GUEST_ELF.as_ptr(),
            GUEST_ELF.len(),
        ))
    }
    .map_err(|e| anyhow::anyhow!("ELF parse error: {:?}", e))?;

    let elf_paddr_base: usize =
        elf.segments()
            .and_then(|segments| {
                segments
                    .iter()
                    .filter(|ph| ph.p_type == PT_LOAD)
                    .map(|ph| ph.p_paddr)
                    .min()
            })
            .ok_or_else(|| anyhow::anyhow!("ELF has no PT_LOAD segments"))? as usize;

    let mut segments = alloc::vec::Vec::new();
    if let Some(hdrs) = elf.segments() {
        for ph in hdrs.iter().filter(|ph| ph.p_type == PT_LOAD) {
            let elf_paddr = ph.p_paddr as usize;

            let relative = elf_paddr
                .checked_sub(elf_paddr_base)
                .ok_or_else(|| anyhow::anyhow!("ELF has no PT_LOAD segments"))?;

            let gpa = GUEST_DRAM_GPA_START
                .checked_add(relative)
                .ok_or_else(|| anyhow::anyhow!("GPA overflow"))?;

            segments.push(LazySegment::new(
                gpa,
                ph.p_memsz as usize,
                ph.p_filesz as usize,
                ph.p_offset as usize,
            ));

            println!("ELF paddr 0x{:x} -> guest GPA 0x{:x}", elf_paddr, gpa);
        }
    }

    /* Copy DTB into guest space */
    let dtb_offset = (GUEST_DRAM_SIZE - GUEST_DTB.len() - 1) & !(PAGE_SIZE - 1);
    let dtb_addr = GUEST_DRAM_GPA_START + dtb_offset;

    {
        let mut lock = LAZY_STATE.lock();
        *lock = Some(LazyState::new(
            segments,
            GUEST_ELF.as_slice(),
            dtb_addr,
            GUEST_DTB.as_slice(),
            0x01000000,
            GUEST_INITRD.as_slice(),
            conf_pool_base,
            conf_pool_base + GUEST_DRAM_SIZE,
            state_addr - pt_addr,
        ));
        println!(
            "[OLORIN] initialized page fault state: 0x{:x} - 0x{:x}",
            conf_pool_base,
            conf_pool_base + GUEST_DRAM_SIZE,
        );
    }

    // Standard TVM Creation (Metadata only, NO MAPPING)
    let attestation = state.attestation_context.compute_next(&[0; 32]);
    let tvm_id = state
        .hypervisor
        .create_tvm(0, attestation, pt_addr, state_addr)?;

    state
        .hypervisor
        .add_tvm_memory_region(tvm_id, GUEST_DRAM_GPA_START, GUEST_DRAM_SIZE)?;

    state
        .hypervisor
        .add_tvm_mmio_region(tvm_id, UART_GPA, UART_HPA, PAGE_SIZE)?;

    // Finalize
    let entry_virtual = elf.ehdr.e_entry as usize;
    let entry_segment = elf
        .segments()
        .and_then(|segments| {
            segments.iter().filter(|p| p.p_type == PT_LOAD).find(|p| {
                let start = p.p_vaddr as usize;
                let end = start + p.p_memsz as usize;
                entry_virtual >= start && entry_virtual < end
            })
        })
        .expect("cannot find entrypoint");

    // Offset of entry within its PT_LOAD segment.
    let entry_offset = entry_virtual - entry_segment.p_vaddr as usize;

    // Offset of the entry segment relative to the ELF physical base.
    let entry_segment_offset = entry_segment.p_paddr as usize - elf_paddr_base;

    let entry_gpa = GUEST_DRAM_GPA_START
        .checked_add(entry_segment_offset)
        .and_then(|address| address.checked_add(entry_offset))
        .ok_or_else(|| anyhow::anyhow!("Entry GPA overflow"))?;
    println!("[OLORIN] Guest entrypoint: 0x{:x}", entry_gpa);
    println!("[OLORIN] Guest dtb at : 0x{:x}", dtb_addr);
    state
        .hypervisor
        .finalize_tvm(tvm_id, entry_gpa, dtb_addr, 0, &state.attestation_context)?;

    Ok(tvm_id)
}

fn bootstrap_load_elf(
    state: &mut TsmState,
    pt_addr: usize,
    state_addr: usize,
    conf_pool_base: usize,
) -> anyhow::Result<usize> {
    let elf = unsafe {
        ElfBytes::<AnyEndian>::minimal_parse(core::slice::from_raw_parts(
            GUEST_ELF.as_ptr(),
            GUEST_ELF.len(),
        ))
    }
    .map_err(|e| anyhow::anyhow!("ELF parse error: {:?}", e))?;

    // 1. Create TVM
    let attestation = state.attestation_context.compute_next(&[0; 32]);
    let tvm_id = state
        .hypervisor
        .create_tvm(0, attestation, pt_addr, state_addr)?;

    // 2. Define Guest RAM - MATCH LINKER SCRIPT (ORIGIN = 0x1000)
    let gpa_base = 0x0;
    let ram_size = 64 * 1024 * 1024;
    state
        .hypervisor
        .add_tvm_memory_region(tvm_id, gpa_base, ram_size)?;

    let segments = elf
        .segments()
        .ok_or_else(|| anyhow::anyhow!("No program headers"))?;
    let mut current_conf_ptr = conf_pool_base;
    let mut highest_gpa_mapped = gpa_base;

    // 3. Load PT_LOAD segments
    for ph in segments.iter().filter(|ph| ph.p_type == PT_LOAD) {
        let p_paddr = ph.p_paddr as usize;
        let p_filesz = ph.p_filesz as usize;
        let p_memsz = ph.p_memsz as usize;
        let p_offset = ph.p_offset as usize;

        // Alignment Math
        let gpa_page_start = p_paddr & !(PAGE_SIZE - 1);
        let offset_in_page = p_paddr - gpa_page_start;
        let num_measured_pages = (offset_in_page + p_filesz + PAGE_SIZE - 1) / PAGE_SIZE;

        let data = GUEST_ELF.as_slice();
        if p_filesz > 0 {
            // FIX: Use an aligned scratchpad to avoid "src addr must be page-aligned" panic
            let layout =
                Layout::from_size_align(num_measured_pages * PAGE_SIZE, PAGE_SIZE).unwrap();
            unsafe {
                let scratchpad = alloc::alloc::alloc_zeroed(layout);
                if scratchpad.is_null() {
                    return Err(anyhow::anyhow!("TSM Out of Memory"));
                }

                // Copy ELF data into scratchpad at the correct sub-page offset
                let src_data = &data[p_offset..p_offset + p_filesz];
                core::ptr::copy_nonoverlapping(
                    src_data.as_ptr(),
                    scratchpad.add(offset_in_page),
                    p_filesz,
                );

                // Map and measure the aligned scratchpad
                state.hypervisor.add_tvm_measured_pages(
                    tvm_id,
                    scratchpad as usize,
                    current_conf_ptr,
                    0, // 4K
                    num_measured_pages,
                    gpa_page_start,
                )?;

                alloc::alloc::dealloc(scratchpad, layout);
            }

            // println!(
            //     "[OLORIN] mapping 0x{:x} - 0x{:x} -> 0x{:x} - 0x{:x}; 0x{:x} - 0x{:x}",
            //     p_paddr,
            //     p_paddr + p_filesz,
            //     gpa_page_start,
            //     gpa_page_start + num_measured_pages * PAGE_SIZE,
            //     current_conf_ptr,
            //     current_conf_ptr + num_measured_pages * PAGE_SIZE
            // );
            current_conf_ptr += num_measured_pages * PAGE_SIZE;
        }

        // Handle .bss suffix within the same segment
        if p_memsz > p_filesz {
            let total_pages = (offset_in_page + p_memsz + PAGE_SIZE - 1) / PAGE_SIZE;
            let zero_pages = total_pages - num_measured_pages;

            if zero_pages > 0 {
                let zero_gpa_start = gpa_page_start + (num_measured_pages * PAGE_SIZE);
                state.hypervisor.add_tvm_zero_pages(
                    tvm_id,
                    current_conf_ptr,
                    0,
                    zero_pages,
                    zero_gpa_start,
                )?;
                current_conf_ptr += zero_pages * PAGE_SIZE;
            }
        }

        // Track the end of mapped memory
        let segment_end =
            gpa_page_start + ((offset_in_page + p_memsz + PAGE_SIZE - 1) & !(PAGE_SIZE - 1));
        highest_gpa_mapped = highest_gpa_mapped.max(segment_end);
    }

    // 4. Map the rest of the 2MB RAM (the stack and heap).
    // Freestanding guests may use memory beyond their PT_LOAD segments.
    let ram_end_gpa = gpa_base + ram_size;
    if highest_gpa_mapped < ram_end_gpa {
        let remaining_pages = (ram_end_gpa - highest_gpa_mapped) / PAGE_SIZE;
        state.hypervisor.add_tvm_zero_pages(
            tvm_id,
            current_conf_ptr,
            0,
            remaining_pages,
            highest_gpa_mapped,
        )?;
    }

    // 5. Finalize TVM
    let entry_point = elf.ehdr.e_entry as usize;
    state
        .hypervisor
        .finalize_tvm(tvm_id, entry_point, 0, 0, &state.attestation_context)?;

    Ok(tvm_id)
}

#[inline(always)]
fn read_cycle() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("csrr {}, cycle", out(reg) value);
    }
    value
}

#[allow(unused)]
#[inline(always)]
fn read_instret() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("csrr {}, instret", out(reg) value);
    }
    value
}

#[allow(unused)]
#[inline(always)]
fn read_time() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("csrr {}, time", out(reg) value);
    }
    value
}
