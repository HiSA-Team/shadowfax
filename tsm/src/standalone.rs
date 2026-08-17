use common::{attestation::DiceLayer, sbi::PAGE_SIZE};
use elf::{abi::PT_LOAD, endian::AnyEndian, ElfBytes};

use crate::{
    hyper::{HypervisorState, LazySegment, LazyState},
    println, TsmState, _secure_init, STATE,
};

/* Guest */
const GUEST_DRAM_GPA_START: usize = 0x20_0000;
const GUEST_DRAM_SIZE: usize = 256 * 1024 * 1024;

const GUEST_INITRD_GPA: usize = 0x0100_0000;

const CONFIDENTIAL_MEMORY_START: usize = 0x94000000;

const UART_GPA: usize = 0x1800_0000;
const UART_HPA: usize = 0x1000_0000;

const GPT_SIZE: usize = 16 * 1024 * 1024;

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
    let tvm_page_table_addr = CONFIDENTIAL_MEMORY_START;
    let tvm_gpt_pages = GPT_SIZE / PAGE_SIZE;
    let tvm_state_addr = CONFIDENTIAL_MEMORY_START + GPT_SIZE;
    let tvm_confidential_pool = tvm_state_addr + PAGE_SIZE;
    let pool_size_pages = GUEST_DRAM_SIZE / PAGE_SIZE;

    state
        .hypervisor
        .add_confidential_pages(tvm_page_table_addr, tvm_gpt_pages)
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
    let tvm_id = {
        #[cfg(feature = "lazy")]
        {
            println!("[OLORIN] lazy mode -> there will be page faults");
            bootstrap_load_elf_lazy(
                state,
                tvm_page_table_addr,
                tvm_state_addr,
                tvm_confidential_pool,
            )
            .expect("Failed to load ELF")
        }

        #[cfg(not(feature = "lazy"))]
        {
            println!("[OLORIN] no lazy mode -> no page faults");
            bootstrap_load_elf(
                state,
                tvm_page_table_addr,
                tvm_state_addr,
                tvm_confidential_pool,
            )
            .expect("Failed to load ELF")
        }
    };

    // 5. Create VCPU (ID 0)
    state
        .hypervisor
        .create_tvm_vcpu(tvm_id, 0, 0)
        .expect("Failed to create VCPU");

    println!("[OLORIN] Bootstrap complete. Entering Guest...");

    // 6. Run it!
    let (vcpu_addr, entry_sepc, entry_arg, resume) = state
        .hypervisor
        .prepare_tvm_vcpu(tvm_id, 0)
        .expect("Failed to prepare VCPU");
    drop(lock);

    unsafe { HypervisorState::enter_prepared_tvm_vcpu(vcpu_addr, entry_sepc, entry_arg, resume) }
}

#[cfg(feature = "lazy")]
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

    let dtb_offset = (GUEST_DRAM_SIZE - GUEST_DTB.len() - 1) & !(PAGE_SIZE - 1);
    let dtb_addr = GUEST_DRAM_GPA_START + dtb_offset;

    // Standard TVM Creation (Metadata only, NO MAPPING)
    let attestation = state.attestation_context.compute_next(&[0; 32]);
    let tvmid = state
        .hypervisor
        .create_tvm(0, attestation, pt_addr, state_addr)?;
    state
        .hypervisor
        .add_tvm_memory_region(tvmid, GUEST_DRAM_GPA_START, GUEST_DRAM_SIZE)?;

    state
        .hypervisor
        .add_tvm_mmio_region(tvmid, UART_GPA, UART_HPA, PAGE_SIZE)?;

    println!(
        "[OLORIN] created TVM memory region: 0x{:x} - 0x{:x}",
        GUEST_DRAM_GPA_START,
        GUEST_DRAM_GPA_START + GUEST_DRAM_SIZE
    );

    println!(
        "[OLORIN] created TVM MMIO memory region: 0x{:x} - 0x{:x}",
        UART_GPA,
        UART_GPA + PAGE_SIZE
    );

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

            segments.push(LazySegment {
                gpa,
                memsz: ph.p_memsz as usize,
                filesz: ph.p_filesz as usize,
                offset: ph.p_offset as usize,
            });

            println!("ELF paddr 0x{:x} -> guest GPA 0x{:x}", elf_paddr, gpa);
        }
    }

    state.hypervisor.set_tvm_lazy_state(
        tvmid,
        LazyState::new(
            segments,
            GUEST_ELF.as_slice(),
            dtb_addr,
            GUEST_DTB.as_slice(),
            GUEST_INITRD_GPA,
            GUEST_INITRD.as_slice(),
            conf_pool_base,
            conf_pool_base + GUEST_DRAM_SIZE,
        ),
    )?;
    println!(
        "[OLORIN] initialized page fault state: 0x{:x} - 0x{:x}",
        conf_pool_base,
        conf_pool_base + GUEST_DRAM_SIZE,
    );

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
        .finalize_tvm(tvmid, entry_gpa, dtb_addr, 0, &state.attestation_context)?;

    Ok(tvmid)
}

fn page_overlaps_source(
    gpa_page: usize,
    gpa_page_end: usize,
    source_gpa: usize,
    source_len: usize,
) -> bool {
    let source_end = source_gpa + source_len;
    gpa_page < source_end && source_gpa < gpa_page_end
}

fn map_page_source(
    state: &mut TsmState,
    tvmid: usize,
    pa: usize,
    gpa_page: usize,
    gpa_page_end: usize,
    source_gpa: usize,
    source_len: usize,
    source_addr: usize,
    source_offset: usize,
) -> anyhow::Result<bool> {
    if !page_overlaps_source(gpa_page, gpa_page_end, source_gpa, source_len) {
        return Ok(false);
    }

    let copy_gpa_start = core::cmp::max(gpa_page, source_gpa);
    let source_offset = source_offset + copy_gpa_start - source_gpa;
    let destination_offset = copy_gpa_start - gpa_page;
    let src_addr = unsafe { (source_addr as *const u8).add(source_offset) } as usize;

    state.hypervisor.add_tvm_measured_pages(
        tvmid,
        src_addr,
        pa + destination_offset,
        0,
        1,
        gpa_page,
    )?;
    // println!("[OLORIN] created mapping: [0x{:x} -> 0x{:x}]", gpa_page, pa);

    Ok(true)
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

    // Standard TVM Creation (Metadata only, NO MAPPING)
    let attestation = state.attestation_context.compute_next(&[0; 32]);
    let tvmid = state
        .hypervisor
        .create_tvm(0, attestation, pt_addr, state_addr)?;
    state
        .hypervisor
        .add_tvm_memory_region(tvmid, GUEST_DRAM_GPA_START, GUEST_DRAM_SIZE)?;

    state
        .hypervisor
        .add_tvm_mmio_region(tvmid, UART_GPA, UART_HPA, PAGE_SIZE)?;

    println!("[OLORIN] created TVM with id {}", tvmid);
    println!(
        "[OLORIN] created TVM memory region: 0x{:x} - 0x{:x}",
        GUEST_DRAM_GPA_START,
        GUEST_DRAM_GPA_START + GUEST_DRAM_SIZE
    );

    println!(
        "[OLORIN] created TVM MMIO memory region: 0x{:x} - 0x{:x}",
        UART_GPA,
        UART_GPA + PAGE_SIZE
    );

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

            segments.push(LazySegment {
                gpa,
                memsz: ph.p_memsz as usize,
                filesz: ph.p_filesz as usize,
                offset: ph.p_offset as usize,
            });
        }
    }

    let mut pa = conf_pool_base;

    let dtb_offset = (GUEST_DRAM_SIZE - GUEST_DTB.len() - 1) & !(PAGE_SIZE - 1);
    let dtb_gpa_base = GUEST_DRAM_GPA_START + dtb_offset;
    let initrd_gpa_base = GUEST_INITRD_GPA;
    let guest_end = GUEST_DRAM_GPA_START + GUEST_DRAM_SIZE;
    let mut gpa_page = GUEST_DRAM_GPA_START;
    while gpa_page < guest_end {
        let gpa_page_end = gpa_page + PAGE_SIZE;
        let page_has_source =
            segments.iter().any(|segment| {
                page_overlaps_source(gpa_page, gpa_page_end, segment.gpa, segment.filesz)
            }) || page_overlaps_source(gpa_page, gpa_page_end, dtb_gpa_base, GUEST_DTB.len())
                || page_overlaps_source(
                    gpa_page,
                    gpa_page_end,
                    initrd_gpa_base,
                    GUEST_INITRD.len(),
                );

        if page_has_source {
            for segment in &segments {
                let seg_start = segment.gpa;
                let seg_end = segment.gpa + segment.memsz;

                if gpa_page_end <= seg_start || gpa_page >= seg_end {
                    continue;
                }

                map_page_source(
                    state,
                    tvmid,
                    pa,
                    gpa_page,
                    gpa_page_end,
                    segment.gpa,
                    segment.filesz,
                    GUEST_ELF.as_ptr() as usize,
                    segment.offset,
                )?;
            }
            map_page_source(
                state,
                tvmid,
                pa,
                gpa_page,
                gpa_page_end,
                dtb_gpa_base,
                GUEST_DTB.len(),
                GUEST_DTB.as_ptr() as usize,
                0,
            )?;
            map_page_source(
                state,
                tvmid,
                pa,
                gpa_page,
                gpa_page_end,
                initrd_gpa_base,
                GUEST_INITRD.len(),
                GUEST_INITRD.as_ptr() as usize,
                0,
            )?;

            pa += PAGE_SIZE;
            gpa_page += PAGE_SIZE;
            continue;
        }

        let zero_gpa_start = gpa_page;
        let mut zero_pages = 0;
        while gpa_page < guest_end {
            let page_end = gpa_page + PAGE_SIZE;
            let has_source =
                segments.iter().any(|segment| {
                    page_overlaps_source(gpa_page, page_end, segment.gpa, segment.filesz)
                }) || page_overlaps_source(gpa_page, page_end, dtb_gpa_base, GUEST_DTB.len())
                    || page_overlaps_source(
                        gpa_page,
                        page_end,
                        initrd_gpa_base,
                        GUEST_INITRD.len(),
                    );
            if has_source {
                break;
            }
            zero_pages += 1;
            gpa_page += PAGE_SIZE;
        }

        state
            .hypervisor
            .add_tvm_zero_pages(tvmid, pa, 0, zero_pages, zero_gpa_start)?;
        pa += zero_pages * PAGE_SIZE;
    }

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
    println!("[OLORIN] Guest dtb at : 0x{:x}", dtb_gpa_base);
    state.hypervisor.finalize_tvm(
        tvmid,
        entry_gpa,
        dtb_gpa_base,
        0,
        &state.attestation_context,
    )?;

    Ok(tvmid)
}

#[allow(unused)]
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
