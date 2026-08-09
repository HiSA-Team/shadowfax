use alloc::vec::Vec;
use common::{
    attestation::{DiceLayer, TvmAttestationContext},
    sbi::{sbi_call, SbiRet, COVG_EXTENSION, PAGE_SIZE},
};
use core::{alloc::Layout, num};
use elf::{abi::PT_LOAD, endian::AnyEndian, ElfBytes};
use riscv::{
    interrupt::Trap,
    register::{
        sepc,
        sstatus::{self, FS, SPP},
        stvec::{self, Stvec},
    },
};
use sha2::{Digest, Sha384};
use spin::Mutex;
use zeroize::Zeroize;

use crate::{
    h_extension::{
        csrs::{hedeleg, henvcfg, hgatp, hideleg, hstatus, htval, vsatp},
        instruction::hfence_gvma_all,
        HvException,
    },
    perf::{self, read_cycle},
    println,
    sbi::{self, handle_covg},
    TsmState, GUEST_DTB, GUEST_ELF, MEASUREMENT,
};

const MIN_PAGE_DIRECTORY_SIZE: usize = 16 * 1024;

const PTE_SIZE: usize = 8;
const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;

// -----------------------------
// Helper functions for SV39
// -----------------------------

/// Return the 3 VPN indices [vpn2, vpn1, vpn0] for SV39.
#[inline(always)]
fn make_vpn_sv39(gpa: usize) -> [usize; 3] {
    [
        (gpa >> 30) & 0x1FF, // VPN[2]
        (gpa >> 21) & 0x1FF, // VPN[1]
        (gpa >> 12) & 0x1FF, // VPN[0]
    ]
}

#[inline(always)]
fn pa_to_ppn(pa: usize) -> u64 {
    (pa as u64) >> 12
}

#[inline(always)]
fn ppn_to_pa(ppn: u64) -> usize {
    (ppn << 12) as usize
}

/// Map a single 4 KiB page in SV39 page tables.
/// Dynamically allocates page tables within the region supplied by the host.
///
/// Memory layout:
///   root_pt + 0x0000: L2 table (root)
///   root_pt + 0x1000: L1 table (shared for all VPN[2]=0)
///   root_pt + 0x2000 + VPN[1] * 0x1000: L0 tables
///
/// Note: This assumes all mappings use VPN[2]=0 (addresses < 1GB)
fn map_4k_leaf(root_pt: usize, page_table_size: usize, gpa: usize, pa: usize, perms: u64) {
    assert_eq!(gpa % PAGE_SIZE, 0, "GPA must be page-aligned");
    assert_eq!(pa % PAGE_SIZE, 0, "PA must be page-aligned");

    let [vpn2, vpn1, vpn0] = make_vpn_sv39(gpa);

    // Level 2 -> Level 1
    let pte2_addr = root_pt + vpn2 * PTE_SIZE;
    let pte2 = unsafe { core::ptr::read_volatile(pte2_addr as *const u64) };

    let l1_base = if pte2 & PTE_V == 0 {
        // L1 table doesn't exist, create it
        let l1_base = root_pt + 0x1000;
        let pte = (pa_to_ppn(l1_base) << 10) | PTE_V;
        unsafe {
            core::ptr::write_volatile(pte2_addr as *mut u64, pte);
        }
        l1_base
    } else {
        // L1 already exists, extract its address
        ppn_to_pa(pte2 >> 10)
    };

    // Level 1 -> Level 0
    let pte1_addr = l1_base + vpn1 * PTE_SIZE;
    let pte1 = unsafe { core::ptr::read_volatile(pte1_addr as *const u64) };

    let l0_base = if pte1 & PTE_V == 0 {
        // L0 table doesn't exist, allocate it
        // Allocate one L0 table for each populated VPN[1].
        let l0_base = root_pt + 0x2000 + (vpn1 * PAGE_SIZE);

        // Check that the host supplied enough space for another L0 table.
        assert!(
            l0_base + PAGE_SIZE <= root_pt + page_table_size,
            "Insufficient space for L0 table at VPN[1]={}",
            vpn1
        );

        let pte = (pa_to_ppn(l0_base) << 10) | PTE_V;
        unsafe {
            core::ptr::write_volatile(pte1_addr as *mut u64, pte);
        }
        l0_base
    } else {
        // L0 already exists
        ppn_to_pa(pte1 >> 10)
    };

    // Level 0 (leaf)
    let pte0_addr = l0_base + vpn0 * PTE_SIZE;
    let leaf = (pa_to_ppn(pa) << 10) | perms | PTE_V | PTE_U;
    unsafe {
        core::ptr::write_volatile(pte0_addr as *mut u64, leaf);
    }
}

/// Translates a Guest Physical Address (GPA) to a Host Physical Address (PA)
/// by walking the SV39 page table structure starting at `root_pt`.
/// Returns `None` if the address is not mapped.
pub fn translate_gpa_to_pa(root_pt: usize, gpa: usize) -> Option<usize> {
    let [vpn2, vpn1, vpn0] = make_vpn_sv39(gpa);

    // --- Level 2 (Root) ---
    // Calculate address of the PTE in the L2 table
    let pte2_addr = root_pt + (vpn2 * 8);
    let pte2 = unsafe { core::ptr::read_volatile(pte2_addr as *const u64) };

    // 1. Check Valid bit
    if (pte2 & PTE_V) == 0 {
        return None; // Page fault (not mapped)
    }

    // 2. Check for Leaf (Huge Page 1GB)
    // If R, W, or X is set, this is a leaf node, not a pointer to the next level.
    if (pte2 & (PTE_R | PTE_W | PTE_X)) != 0 {
        // PPN holds the 1GB aligned base address
        let ppn = (pte2 >> 10) & 0x003F_FFFF_FFFF_FFFF;
        // PA = (PPN << 12) | Offset within 1GB (30 bits)
        return Some(ppn_to_pa(ppn) | (gpa & 0x3FFF_FFFF));
    }

    // --- Level 1 ---
    // pte2 was a pointer to the L1 table
    let l1_base = ppn_to_pa(pte2 >> 10);
    let pte1_addr = l1_base + (vpn1 * 8);
    let pte1 = unsafe { core::ptr::read_volatile(pte1_addr as *const u64) };

    if (pte1 & PTE_V) == 0 {
        return None;
    }

    // Check for Leaf (Huge Page 2MB)
    if (pte1 & (PTE_R | PTE_W | PTE_X)) != 0 {
        let ppn = (pte1 >> 10) & 0x003F_FFFF_FFFF_FFFF;
        // PA = (PPN << 12) | Offset within 2MB (21 bits)
        return Some(ppn_to_pa(ppn) | (gpa & 0x1F_FFFF));
    }

    // --- Level 0 (4KB Page) ---
    // pte1 was a pointer to the L0 table
    let l0_base = ppn_to_pa(pte1 >> 10);
    let pte0_addr = l0_base + (vpn0 * 8);
    let pte0 = unsafe { core::ptr::read_volatile(pte0_addr as *const u64) };

    if (pte0 & PTE_V) == 0 {
        return None;
    }

    // This must be a leaf (standard 4KB page)
    if (pte0 & (PTE_R | PTE_W | PTE_X)) == 0 {
        return None; // Invalid format: L0 PTE must be a leaf
    }

    let ppn = (pte0 >> 10) & 0x003F_FFFF_FFFF_FFFF;
    // PA = (PPN << 12) | Offset within 4KB (12 bits)
    Some(ppn_to_pa(ppn) | (gpa & 0xFFF))
}

/// Map a contiguous region of memory (multiple 4KB pages).
fn map_region(
    root_pt: usize,
    page_table_size: usize,
    gpa_base: usize,
    pa_base: usize,
    num_pages: usize,
    perms: u64,
) {
    for i in 0..num_pages {
        // TODO align GPA to PAGE
        let gpa = gpa_base + i * PAGE_SIZE;
        let pa = pa_base + i * PAGE_SIZE;
        map_4k_leaf(root_pt, page_table_size, gpa, pa, perms);
    }
}

/// Reads `len` bytes from Guest Physical Address `gpa` into `buf`.
/// Returns error if translation fails or crosses page boundary.
pub fn read_guest_memory(root_pt: usize, gpa: usize, buf: &mut [u8]) -> Result<(), ()> {
    // 1. Check for page crossing (Simplified: fail if it crosses)
    if (gpa & 0xFFF) + buf.len() > 4096 {
        return Err(());
    }

    // 2. Translate
    let host_pa = translate_gpa_to_pa(root_pt, gpa).ok_or(())?;

    // 3. Copy
    unsafe {
        let src = host_pa as *const u8;
        core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), buf.len());
    }
    Ok(())
}

/// Writes `data` to Guest Physical Address `gpa`.
pub fn write_guest_memory(root_pt: usize, gpa: usize, data: &[u8]) -> Result<(), ()> {
    if (gpa & 0xFFF) + data.len() > 4096 {
        return Err(());
    }

    let host_pa = translate_gpa_to_pa(root_pt, gpa).ok_or(())?;

    unsafe {
        let dst = host_pa as *mut u8;
        core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
    }
    Ok(())
}

// -----------------------------
// Core TSM structures
// -----------------------------

#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub guest_gpa_base: usize,
    pub num_pages: usize,
}

pub struct HypervisorState {
    pub tvm: Option<Tvm>,
    /* Base page address, num pages, vmid */
    confidential_memory: Vec<(usize, usize, Option<usize>)>,
}

impl HypervisorState {
    pub fn new() -> Self {
        Self {
            tvm: None,
            confidential_memory: Vec::new(),
        }
    }
    // TODO: Zero out the confidential pages
    pub fn add_confidential_pages(
        &mut self,
        base_page_addr: usize,
        num_pages: usize,
    ) -> anyhow::Result<()> {
        self.confidential_memory
            .push((base_page_addr, num_pages, None));
        Ok(())
    }

    pub fn create_tvm(
        &mut self,
        attestation_context: TvmAttestationContext,
        page_table_addr: usize,
        state_addr: usize,
    ) -> anyhow::Result<usize> {
        if self.tvm.is_some() {
            anyhow::bail!("already created tvm");
        }

        if page_table_addr % MIN_PAGE_DIRECTORY_SIZE != 0 {
            anyhow::bail!("page table addr must be 16KB-aligned");
        }

        let page_table_size = state_addr
            .checked_sub(page_table_addr)
            .ok_or_else(|| anyhow::anyhow!("state address precedes page table"))?;
        if page_table_size < MIN_PAGE_DIRECTORY_SIZE || page_table_size % PAGE_SIZE != 0 {
            anyhow::bail!("invalid page table size");
        }

        let pd_block_idx = self
            .find_confidential_block_idx_covering(page_table_addr, page_table_size)
            .ok_or_else(|| anyhow::anyhow!("page directory addr not in confidential memory"))?;

        let state_block_idx = self
            .find_confidential_block_idx_covering(state_addr, PAGE_SIZE)
            .ok_or_else(|| anyhow::anyhow!("state addr not in confidential memory"))?;

        {
            let (_base, _npages, owner) = self
                .confidential_memory
                .get_mut(pd_block_idx)
                .ok_or_else(|| anyhow::anyhow!("invalid pd block idx"))?;
            *owner = Some(1);
        }
        {
            let (_base, _npages, owner) = self
                .confidential_memory
                .get_mut(state_block_idx)
                .ok_or_else(|| anyhow::anyhow!("invalid state block idx"))?;
            *owner = Some(1);
        }

        unsafe {
            let ptr = page_table_addr as *mut u8;
            core::ptr::write_bytes(ptr, 0, page_table_size);
        }

        let tvm = Tvm::new(
            attestation_context,
            page_table_addr,
            page_table_size,
            state_addr,
        );
        let tvm_id = tvm.id;
        self.tvm = Some(tvm);
        Ok(tvm_id)
    }

    pub fn finalize_tvm(
        &mut self,
        _tvm_id: usize,
        entry_sepc: usize,
        entry_arg: usize,
        tvm_identity_addr: usize,
    ) -> anyhow::Result<()> {
        if let Some(tvm) = &mut self.tvm {
            tvm.finalize(entry_sepc, entry_arg, tvm_identity_addr);
        } else {
            anyhow::bail!("no tvm present");
        }

        Ok(())
    }

    pub fn destroy_tvm(&mut self) -> anyhow::Result<()> {
        if let Some(tvm) = &self.tvm {
            unsafe {
                let ptr = tvm.page_table_addr as *mut u8;
                core::ptr::write_bytes(ptr, 0, tvm.page_table_size);
            }
        }
        self.tvm = None;
        Ok(())
    }

    pub fn add_tvm_memory_region(
        &mut self,
        tvm_id: usize,
        tvm_gpa_addr: usize,
        region_len_bytes: usize,
    ) -> anyhow::Result<()> {
        if self.tvm.is_none() {
            anyhow::bail!("no tvm present");
        }

        let t = self.tvm.as_mut().unwrap();
        if t.id != tvm_id {
            anyhow::bail!("tvm id mismatch");
        }

        match t.state_enum {
            TvmState::TvmInitializing => {}
            _ => anyhow::bail!("cannot add memory region unless TVM_INITIALIZING"),
        }

        if (tvm_gpa_addr % PAGE_SIZE) != 0
            || (region_len_bytes % PAGE_SIZE) != 0
            || region_len_bytes == 0
        {
            anyhow::bail!("tvm_gpa_addr and region_len must be 4KB-aligned and non-zero");
        }

        let num_pages = region_len_bytes / PAGE_SIZE;
        let new_a = tvm_gpa_addr;
        let new_b = tvm_gpa_addr + region_len_bytes;

        for r in t.memory_regions.iter() {
            let r_a = r.guest_gpa_base;
            let r_b = r.guest_gpa_base + r.num_pages * PAGE_SIZE;
            if !(new_b <= r_a || r_b <= new_a) {
                anyhow::bail!("region overlap with existing region");
            }
        }

        t.memory_regions.push(MemoryRegion {
            guest_gpa_base: tvm_gpa_addr,
            num_pages,
        });
        Ok(())
    }

    pub fn add_tvm_mmio_region(
        &mut self,
        tvm_id: usize,
        guest_gpa: usize,
        host_pa: usize,
        size: usize,
    ) -> anyhow::Result<()> {
        if guest_gpa % PAGE_SIZE != 0
            || host_pa % PAGE_SIZE != 0
            || size == 0
            || size % PAGE_SIZE != 0
        {
            anyhow::bail!("MMIO addresses and size must be page-aligned");
        }

        let tvm = self
            .tvm
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no TVM present"))?;

        if tvm.id != tvm_id {
            anyhow::bail!("TVM ID mismatch");
        }

        match tvm.state_enum {
            TvmState::TvmInitializing => {}
            _ => anyhow::bail!("cannot add MMIO after finalization"),
        }

        // Check that the MMIO GPA does not overlap guest RAM.
        let mmio_end = guest_gpa + size;

        for region in &tvm.memory_regions {
            let region_start = region.guest_gpa_base;
            let region_end = region_start + region.num_pages * PAGE_SIZE;

            if guest_gpa < region_end && region_start < mmio_end {
                anyhow::bail!("MMIO overlaps an existing guest region");
            }
        }

        map_region(
            tvm.page_table_addr,
            tvm.page_table_size,
            guest_gpa,
            host_pa,
            size / PAGE_SIZE,
            PTE_R | PTE_W | PTE_U | PTE_A | PTE_D,
        );

        Ok(())
    }

    pub fn add_tvm_measured_pages(
        &mut self,
        tvm_id: usize,
        source_addr: usize,
        dest_addr: usize,
        tsm_page_type: usize,
        num_pages: usize,
        tvm_guest_gpa: usize,
    ) -> anyhow::Result<()> {
        if self.tvm.is_none() {
            anyhow::bail!("no tvm present");
        }

        let tvm = self.tvm.as_mut().unwrap();
        if tvm.id != tvm_id {
            anyhow::bail!("tvm id mismatch");
        }

        match tvm.state_enum {
            TvmState::TvmInitializing => {}
            _ => anyhow::bail!("cannot add memory region unless TVM_INITIALIZING"),
        }

        assert_eq!(tsm_page_type, 0, "accepting 4k pages for now");

        // if (source_addr % PAGE_SIZE) != 0
        if (dest_addr % PAGE_SIZE) != 0 || (tvm_guest_gpa % PAGE_SIZE) != 0 {
            anyhow::bail!("all addresses must be page-aligned");
        }

        // Verify the GPA range falls within a defined memory region
        let gpa_end = tvm_guest_gpa + num_pages * PAGE_SIZE;
        let mut found_region = false;

        for r in tvm.memory_regions.iter() {
            let r_start = r.guest_gpa_base;
            let r_end = r.guest_gpa_base + r.num_pages * PAGE_SIZE;

            if tvm_guest_gpa >= r_start && gpa_end <= r_end {
                found_region = true;
                break;
            }
        }

        if !found_region {
            anyhow::bail!(
                "GPA range 0x{:x}-0x{:x} not within any memory region",
                tvm_guest_gpa,
                gpa_end
            );
        }

        // Verify dest_addr is in confidential memory
        let dest_end = dest_addr + num_pages * PAGE_SIZE;
        let mut in_confidential = false;

        for (base, npages, owner) in self.confidential_memory.iter() {
            let conf_start = *base;
            let conf_end = base + npages * PAGE_SIZE;

            if dest_addr >= conf_start && dest_end <= conf_end {
                // Check if already owned by this TVM
                if owner.is_some() && *owner != Some(tvm_id) {
                    anyhow::bail!("confidential memory already owned by another TVM");
                }
                in_confidential = true;
                break;
            }
        }

        if !in_confidential {
            anyhow::bail!("dest_addr not in confidential memory");
        }

        // Copy the data in confidential memory and extend the measurement
        unsafe {
            let src_ptr = source_addr as *const u8;
            let dst_ptr = dest_addr as *mut u8;
            let bytes = num_pages * PAGE_SIZE;
            core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, bytes);

            let content = core::slice::from_raw_parts(src_ptr, bytes);
            tvm.extend_measure(content);
        }

        // Map each page in the TVM's page table
        map_region(
            tvm.page_table_addr,
            tvm.page_table_size,
            tvm_guest_gpa,
            dest_addr,
            num_pages,
            PTE_R | PTE_W | PTE_X | PTE_U,
        );

        Ok(())
    }

    pub fn add_tvm_zero_pages(
        &mut self,
        tvm_id: usize,
        base_page_address: usize,
        tsm_page_type: usize,
        num_pages: usize,
        tvm_base_page_address: usize,
    ) -> anyhow::Result<()> {
        if self.tvm.is_none() {
            anyhow::bail!("no tvm present");
        }
        let tvm = self.tvm.as_mut().unwrap();
        if tvm.id != tvm_id {
            anyhow::bail!("tvm id mismatch");
        }

        assert_eq!(tsm_page_type, 0, "accepting 4k pages for now");
        if (base_page_address % PAGE_SIZE) != 0 || (tvm_base_page_address % PAGE_SIZE) != 0 {
            anyhow::bail!("all addresses must be page-aligned");
        }
        let mut in_confidential = false;

        let dest_end = base_page_address + num_pages * PAGE_SIZE;
        for (base, npages, owner) in self.confidential_memory.iter() {
            let conf_start = *base;
            let conf_end = base + npages * PAGE_SIZE;

            if base_page_address >= conf_start && dest_end <= conf_end {
                // Check if already owned by this TVM
                if owner.is_some() && *owner != Some(tvm_id) {
                    anyhow::bail!("confidential memory already owned by another TVM");
                }
                in_confidential = true;
                break;
            }
        }
        if !in_confidential {
            anyhow::bail!("dest_addr not in confidential memory");
        }

        // Verify the GPA range falls within a defined memory region
        let gpa_end = tvm_base_page_address + num_pages * PAGE_SIZE;
        let mut found_region = false;

        for r in tvm.memory_regions.iter() {
            let r_start = r.guest_gpa_base;
            let r_end = r.guest_gpa_base + r.num_pages * PAGE_SIZE;

            if tvm_base_page_address >= r_start && gpa_end <= r_end {
                found_region = true;
                break;
            }
        }

        if !found_region {
            anyhow::bail!(
                "GPA range 0x{:x}-0x{:x} not within any memory region",
                tvm_base_page_address,
                gpa_end
            );
        }

        map_region(
            tvm.page_table_addr,
            tvm.page_table_size,
            tvm_base_page_address,
            base_page_address,
            num_pages,
            PTE_R | PTE_W | PTE_X | PTE_U,
        );
        Ok(())
    }

    pub fn create_tvm_vcpu(
        &mut self,
        tvm_id: usize,
        tvm_vcpu_id: usize,
        _tvm_state_page_addr: usize,
    ) -> anyhow::Result<()> {
        if self.tvm.is_none() {
            anyhow::bail!("no tvm present");
        }

        let tvm = self.tvm.as_mut().unwrap();
        if tvm.id != tvm_id {
            anyhow::bail!("tvm id mismatch");
        }

        tvm.vcpu = Some(TvmVcpuState::new(tvm_vcpu_id));
        Ok(())
    }

    pub fn run_tvm_vcpu(&self, tvm_id: usize, _vcpu_id: usize) -> anyhow::Result<!> {
        if self.tvm.is_none() {
            anyhow::bail!("no tvm present");
        }

        let tvm = self.tvm.as_ref().unwrap();
        if tvm.id != tvm_id {
            anyhow::bail!("tvm id mismatch");
        }

        if tvm.vcpu.is_none() {
            anyhow::bail!("no vcpu present");
        }
        let vcpu = tvm.vcpu.as_ref().unwrap();

        match tvm.state_enum {
            TvmState::TvmRunnable => {}
            _ => anyhow::bail!("TVM must be in runnable state"),
        }

        // Setup H-extension for guest execution
        self.setup_h_extension(&tvm)?;

        unsafe { vcpu.enter(tvm.entry_sepc, tvm.entry_arg) }
    }

    pub fn reclaim_pages(
        &mut self,
        base_page_address: usize,
        num_pages: usize,
    ) -> anyhow::Result<()> {
        if self.tvm.is_some() {
            anyhow::bail!("TVM is still running");
        }

        let idx = self
            .confidential_memory
            .iter()
            .position(|(addr, npages, _)| *addr == base_page_address && *npages == num_pages)
            .ok_or_else(|| anyhow::anyhow!("No matching memory block"))?;

        let total_bytes = num_pages * PAGE_SIZE;

        unsafe {
            let slice = core::slice::from_raw_parts_mut(base_page_address as *mut u8, total_bytes);
            // This ensures the compiler doesn't optimize away the zeroing operation
            slice.zeroize();
        }
        self.confidential_memory.remove(idx);

        Ok(())
    }

    /// Setup H-extension CSRs for guest execution
    fn setup_h_extension(&self, tvm: &Tvm) -> anyhow::Result<()> {
        // Disable VS-mode address translation (guest manages its own)
        vsatp::write(0);

        // Setup guest physical address translation (G-stage)
        hgatp::set(hgatp::Mode::Sv39x4, 0, tvm.page_table_addr >> 12);
        let guest_page_faults = (1usize << 12)  // Instruction page fault
      | (1usize << 13) // Load page fault
      | (1usize << 15); // Store/AMO page fault

        hedeleg::write(guest_page_faults);
        // Delegate the virtual supervisor timer interrupt to VS.
        // VSTIP occupies bit 6 in hideleg/hip.
        hideleg::write(1 << 6);

        hfence_gvma_all();

        Ok(())
    }

    /// Helper to find which confidential memory block contains an address range
    fn find_confidential_block_idx_covering(&self, addr: usize, size: usize) -> Option<usize> {
        let addr_end = addr + size;

        for (idx, (base, npages, _)) in self.confidential_memory.iter().enumerate() {
            let block_start = *base;
            let block_end = base + npages * PAGE_SIZE;

            if addr >= block_start && addr_end <= block_end {
                return Some(idx);
            }
        }
        None
    }
}

#[repr(C)]
pub struct Tvm {
    id: usize,
    page_table_addr: usize,
    page_table_size: usize,
    state_addr: usize,
    memory_regions: Vec<MemoryRegion>,
    state_enum: TvmState,
    vcpu: Option<TvmVcpuState>,
    entry_sepc: usize,
    entry_arg: usize,
    tvm_identity_addr: usize,
    hasher: sha2::Sha384,
    measure: Vec<u8>,
    attestation_context: TvmAttestationContext,
}

impl Tvm {
    fn new(
        attestation_context: TvmAttestationContext,
        page_table_addr: usize,
        page_table_size: usize,
        state_addr: usize,
    ) -> Self {
        Self {
            id: 1,
            page_table_addr,
            page_table_size,
            state_addr,
            memory_regions: Vec::new(),
            state_enum: TvmState::TvmInitializing,
            vcpu: None,
            entry_sepc: 0,
            entry_arg: 0,
            tvm_identity_addr: 0,
            hasher: Sha384::new(),
            measure: Vec::new(),
            attestation_context,
        }
    }

    fn finalize(&mut self, entry_sepc: usize, entry_arg: usize, tvm_identity_addr: usize) {
        // Save entry point
        self.entry_sepc = entry_sepc;
        self.entry_arg = entry_arg;
        self.tvm_identity_addr = tvm_identity_addr;

        // Mark the TVM in a runnable state
        self.state_enum = TvmState::TvmRunnable;

        // Finalize the Measurement
        let old_hasher = core::mem::take(&mut self.hasher);
        self.measure = old_hasher.finalize().to_vec();
        self.hasher = Sha384::new();
        let mut lock = MEASUREMENT.lock();
        lock.replace(self.measure.clone());
    }

    fn extend_measure(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    pub fn get_measure(&self) -> Vec<u8> {
        self.measure.clone()
    }
}

#[derive(Clone)]
enum TvmState {
    TvmInitializing = 0,
    TvmRunnable = 1,
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct VmTrapContext {
    // Guest registers x0-x31 (Offset 0-248)
    // We save x0 as a placeholder to keep indexing simple: regs[i] == x(i)
    pub regs: [usize; 32],
    // Hypervisor Stack Pointer (Offset 256)
    pub hs_sp: usize,
}

#[repr(C, align(4))]
struct TvmVcpuState {
    regs: [usize; 32],
    sstatus: usize,
    stvec: usize,
    sip: usize,
    satp: usize,
    sepc: usize,
    scause: usize,
    stval: usize,
    trap_ctx: VmTrapContext,
    // Hypervisor scratch stack (grows downward from end)
    hs_scratch_stack: [u8; 1024 * 128],
}

impl TvmVcpuState {
    fn new(id: usize) -> Self {
        let mut vcpu = Self {
            regs: [0; 32],
            sstatus: 0,
            stvec: 0,
            sip: 0,
            satp: 0,
            sepc: 0,
            scause: 0,
            stval: 0,
            trap_ctx: VmTrapContext {
                regs: [0; 32],
                hs_sp: 0,
            },
            hs_scratch_stack: [0; 1024 * 128],
        };
        // We write vhartid in a0
        vcpu.regs[10] = id;
        vcpu
    }

    unsafe fn enter(&self, entry_sepc: usize, entry_arg: usize) -> ! {
        let ctx = &self.trap_ctx as *const VmTrapContext as usize;

        // Calculate HS stack top (grows downward, so point to end of array)
        let hs_stack_top = self.hs_scratch_stack.as_ptr() as usize + self.hs_scratch_stack.len();

        // Initialize trap context
        let trap_ctx_mut = ctx as *mut VmTrapContext;
        (*trap_ctx_mut).hs_sp = hs_stack_top;

        // sscratch = &VmTrapContext
        core::arch::asm!("csrw sscratch, {}", in(reg) ctx);

        sstatus::set_sum(); // Allow supervisor to access user pages
        sstatus::set_spp(SPP::Supervisor); // Return to S-mode (VS-mode with SPV=1)
        sstatus::set_sie(); // Enable interrupts
        sstatus::set_fs(FS::Initial); // Enable FP state
        henvcfg::set_cbze(); // Allow cbo.zero
        henvcfg::set_cbcfe(); // Allow cbo.clean/cbo.flush
        henvcfg::set_stce(); // Allow VS-level time-comparator access

        // Hypervisor trap handler
        stvec::write(Stvec::from_bits(hyper_trap as *const fn() as usize));
        // Bit 1 (TM) allows access to the 'time' CSR
        // Bit 0 (CY) allows access to 'cycle'
        // Bit 2 (IR) allows access to 'instret'
        let hcounteren_val: usize = 0b111;
        core::arch::asm!("csrw hcounteren, {}", in(reg) hcounteren_val);
        // Enable virtualization (SPV=1 means we enter VS-mode on sret)
        hstatus::set_spv();

        // Set guest PC
        sepc::write(entry_sepc);

        // TODO: restore vCPU context
        core::arch::asm!(
            r#"
                fence.i
                sret
            "#,
            in("a0") 0usize,
            in("a1") entry_arg,
            options(readonly, noreturn, nostack)
        )
    }
}

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

                    // 1.Check if the call was a CoVE-G
                    let sbi_ret = if regs[17] == COVG_EXTENSION {
                        handle_covg(
                            regs[17],
                            regs[16],
                            &[regs[10], regs[11], regs[12], regs[13], regs[14], regs[15]],
                        )
                    } else {
                        sbi_call(
                            regs[17],
                            regs[16],
                            &[regs[10], regs[11], regs[12], regs[13], regs[14], regs[15]],
                        )
                    };
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

// Track ELF segments to know what to copy where
struct LazySegment {
    gpa: usize,
    memsz: usize,
    filesz: usize,
    offset: usize,
}

// Global state accessible by the trap handler
struct LazyState {
    // elf
    segments: Vec<LazySegment>,
    elf_data: &'static [u8],

    // dtb
    dtb_gpa: usize,
    dtb_data: &'static [u8],

    // // initrd
    // initrd_gpa: usize,
    // initrd_data: &'static [u8],
    next_free_phys: usize, // Simple bump allocator for physical pages
    phys_limit: usize,
    page_table_size: usize,
}

// Mutex to safely access this from the trap handler
static LAZY_STATE: Mutex<Option<LazyState>> = Mutex::new(None);
static mut PAGE_FAULT_COUNTER: usize = 0;

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

        // let initrd_start = lazy.initrd_gpa;
        // let initrd_end = lazy.initrd_gpa + lazy.initrd_data.len();
        // if gpa_page < initrd_end && initrd_start < gpa_page_end {
        //     let copy_gpa_start = core::cmp::max(gpa_page, initrd_start);
        //     let copy_gpa_end = core::cmp::min(gpa_page_end, initrd_end);
        //
        //     let source_offset = copy_gpa_start - initrd_start;
        //     let destination_offset = copy_gpa_start - gpa_page;
        //     let copy_length = copy_gpa_end - copy_gpa_start;
        //
        //     unsafe {
        //         core::ptr::copy_nonoverlapping(
        //             lazy.initrd_data.as_ptr().add(source_offset),
        //             (pa as *mut u8).add(destination_offset),
        //             copy_length,
        //         );
        //     }
        // }

        // unsafe {
        //     PAGE_FAULT_COUNTER += 1;
        //     let a = PAGE_FAULT_COUNTER;
        //     println!("PAGE_FAULT_COUNTER = {}", a);
        // }
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
    // let cycle_end = read_cycle();
    // println!("pfaultcycle = {}", cycle_end - cycle_start);
}

const GUEST_DRAM_SIZE: usize = 64 * 1024 * 1024;
const GUEST_DRAM_GPA_START: usize = 0x20_0000;
const GUEST_DRAM_GPA_END: usize = 0x20_0000 + GUEST_DRAM_SIZE;

pub fn bootstrap_load_elf_lazy(
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

    let mut segments = Vec::new();
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

    /* Copy DTB into guest space */
    let dtb_offset = (GUEST_DRAM_SIZE - GUEST_DTB.len() - 1) & !(PAGE_SIZE - 1);
    let dtb_addr = GUEST_DRAM_GPA_START + dtb_offset;

    {
        let mut lock = LAZY_STATE.lock();
        *lock = Some(LazyState {
            segments,
            elf_data: GUEST_ELF.as_slice(),

            dtb_gpa: dtb_addr,
            dtb_data: GUEST_DTB.as_slice(),

            // initrd_gpa: 0x01000000,
            // initrd_data: GUEST_INITRD.as_slice(),
            next_free_phys: conf_pool_base,
            phys_limit: conf_pool_base + GUEST_DRAM_SIZE,
            page_table_size: state_addr - pt_addr,
        });
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
        .create_tvm(attestation, pt_addr, state_addr)?;

    state
        .hypervisor
        .add_tvm_memory_region(tvm_id, GUEST_DRAM_GPA_START, GUEST_DRAM_SIZE)?;

    const UART_GPA: usize = 0x0500_0000;
    const UART_HPA: usize = 0x1000_0000;

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
        .finalize_tvm(tvm_id, entry_gpa, dtb_addr, 0)?;

    Ok(tvm_id)
}

pub fn bootstrap_load_elf(
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
        .create_tvm(attestation, pt_addr, state_addr)?;

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
                    anyhow::bail!("TSM Out of Memory");
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

    // 4. Map the rest of the 2MB RAM (The Stack and Heap)
    // This is critical. If CoreMark allocates the list outside PT_LOAD,
    // it will be NULL or Fault unless we map the remaining RAM here.
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
    state.hypervisor.finalize_tvm(tvm_id, entry_point, 0, 0)?;

    Ok(tvm_id)
}
