use common::{
    attestation::{DiceLayer, TvmAttestationContext},
    sbi::PAGE_SIZE,
};
use core::alloc::Layout;
use elf::{abi::PT_LOAD, endian::AnyEndian, ElfBytes};
use heapless::Vec;
use riscv::register::{
    sepc,
    sstatus::{self, FS, SPP},
    stvec::{self, Stvec},
};
use sha2::{Digest, Sha384};
use zeroize::Zeroize;

use crate::{
    constants::{
        GUEST_DRAM_GPA_START, GUEST_DRAM_SIZE, MAX_TVM_MEMORY_REGIONS, MIN_PAGE_DIRECTORY_SIZE,
        PTE_A, PTE_D, PTE_R, PTE_SIZE, PTE_U, PTE_V, PTE_W, PTE_X, UART_GPA, UART_HPA,
    },
    h_extension::{
        csrs::{hedeleg, henvcfg, hgatp, hideleg, hstatus, vsatp},
        instruction::hfence_gvma_all,
    },
    println,
    trap::{hyper_trap, LazySegment, LazyState, VmTrapContext, LAZY_STATE},
    TsmState, GUEST_DTB, GUEST_ELF, GUEST_INITRD, MEASUREMENT,
};

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
pub fn map_4k_leaf(root_pt: usize, page_table_size: usize, gpa: usize, pa: usize, perms: u64) {
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

struct PhysicalMemory {
    base_address: usize,
    size: usize,
    guest_id: Option<usize>,
}

pub struct HypervisorState {
    pub tvm: Option<Tvm>,
    /* Base page address, num pages, vmid */
    confidential_memory: Vec<PhysicalMemory, MAX_TVM_MEMORY_REGIONS>,
    run_return_fid: Option<usize>,
}

impl HypervisorState {
    pub fn new() -> Self {
        Self {
            tvm: None,
            confidential_memory: Vec::new(),
            run_return_fid: None,
        }
    }
    // TODO: Zero out the confidential pages
    pub fn add_confidential_pages(
        &mut self,
        base_address: usize,
        num_pages: usize,
    ) -> anyhow::Result<()> {
        self.confidential_memory
            .push(PhysicalMemory {
                base_address,
                size: num_pages * PAGE_SIZE,
                guest_id: None,
            })
            .map_err(|_| anyhow::anyhow!("too many memory regions"))
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
            let PhysicalMemory { guest_id, .. } = self
                .confidential_memory
                .get_mut(pd_block_idx)
                .ok_or_else(|| anyhow::anyhow!("invalid pd block idx"))?;
            *guest_id = Some(1);
        }
        {
            let PhysicalMemory { guest_id, .. } = self
                .confidential_memory
                .get_mut(state_block_idx)
                .ok_or_else(|| anyhow::anyhow!("invalid state block idx"))?;
            *guest_id = Some(1);
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

    pub fn destroy_tvm(&mut self, tvmid: usize) -> anyhow::Result<()> {
        match &self.tvm {
            Some(tvm) => {
                if tvm.id != tvmid {
                    return Err(anyhow::anyhow!("invalid tvm id"));
                }
                match tvm.state_enum {
                    TvmState::TvmStopped => {
                        unsafe {
                            let ptr = tvm.page_table_addr as *mut u8;
                            let v = core::slice::from_raw_parts_mut(ptr, tvm.page_table_size);
                            v.zeroize();
                        }
                        self.tvm = None;
                        Ok(())
                    }
                    _ => Err(anyhow::anyhow!("tvm is not stopped")),
                }
            }
            None => Err(anyhow::anyhow!("no tvm available")),
        }
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

        t.memory_regions
            .push(MemoryRegion {
                guest_gpa_base: tvm_gpa_addr,
                num_pages,
            })
            .map_err(|_| anyhow::anyhow!("cannot push confidential memory regions"))
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

        for PhysicalMemory {
            base_address,
            size,
            guest_id,
        } in self.confidential_memory.iter()
        {
            let conf_start = *base_address;
            let conf_end = base_address + size;

            if dest_addr >= conf_start && dest_end <= conf_end {
                // Check if already owned by this TVM
                if guest_id.is_some() && *guest_id != Some(tvm_id) {
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
        for PhysicalMemory {
            base_address,
            size,
            guest_id,
        } in self.confidential_memory.iter()
        {
            let conf_start = *base_address;
            let conf_end = base_address + size;

            if base_page_address >= conf_start && dest_end <= conf_end {
                // Check if already owned by this TVM
                if guest_id.is_some() && *guest_id != Some(tvm_id) {
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

    pub fn prepare_tvm_vcpu(
        &mut self,
        tvm_id: usize,
        _vcpu_id: usize,
        return_fid: usize,
    ) -> anyhow::Result<(usize, usize, usize)> {
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

        self.run_return_fid = Some(return_fid);

        Ok((
            vcpu as *const TvmVcpuState as usize,
            tvm.entry_sepc,
            tvm.entry_arg,
        ))
    }

    pub unsafe fn enter_prepared_tvm_vcpu(
        vcpu_addr: usize,
        entry_sepc: usize,
        entry_arg: usize,
    ) -> ! {
        (*(vcpu_addr as *const TvmVcpuState)).enter(entry_sepc, entry_arg)
    }

    pub fn guest_shutdown(&mut self) -> anyhow::Result<usize> {
        let tvm = self
            .tvm
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no TVM present"))?;

        match tvm.state_enum {
            TvmState::TvmRunnable => {
                tvm.state_enum = TvmState::TvmStopped;
            }
            _ => anyhow::bail!("TVM is not running"),
        }

        self.run_return_fid
            .take()
            .ok_or_else(|| anyhow::anyhow!("no active RUN_TVM_VCPU call"))
    }

    pub fn reclaim_pages(
        &mut self,
        base_page_address: usize,
        num_pages: usize,
    ) -> anyhow::Result<()> {
        if self.tvm.is_some() {
            return Err(anyhow::anyhow!("TVM is still running"));
        }

        let idx = self
            .confidential_memory
            .iter()
            .position(|pm| {
                pm.base_address == base_page_address && (pm.size / PAGE_SIZE) == num_pages
            })
            .ok_or_else(|| anyhow::anyhow!("No matching memory block"))?;

        let total_bytes = num_pages * PAGE_SIZE;

        self.confidential_memory.remove(idx);

        Ok(())
    }

    /// Setup H-extension CSRs for guest execution
    fn setup_h_extension(&self, tvm: &Tvm) -> anyhow::Result<()> {
        // Disable VS-mode address translation (guest manages its own)
        vsatp::write(0);

        // Setup guest physical address translation (G-stage)
        hgatp::set(hgatp::Mode::Sv39x4, 0, tvm.page_table_addr >> 12);
        let guest_exceptions = (1usize << 0)  // Instruction-address misaligned
            | (1usize << 2)  // Illegal instruction
            | (1usize << 3)  // Breakpoint
            | (1usize << 4)  // Load-address misaligned
            | (1usize << 6)  // Store-address misaligned
            | (1usize << 8)  // U-mode ecall
            | (1usize << 12) // Instruction page fault
            | (1usize << 13) // Load page fault
            | (1usize << 15); // Store page fault

        hedeleg::write(guest_exceptions);
        // Delegate the virtual supervisor timer interrupt to VS.
        // VSTIP occupies bit 6 in hideleg/hip.
        hideleg::write(1 << 6);

        hfence_gvma_all();

        Ok(())
    }

    /// Helper to find which confidential memory block contains an address range
    fn find_confidential_block_idx_covering(&self, addr: usize, size: usize) -> Option<usize> {
        let addr_end = addr + size;

        for (
            idx,
            PhysicalMemory {
                base_address, size, ..
            },
        ) in self.confidential_memory.iter().enumerate()
        {
            let block_start = *base_address;
            let block_end = base_address + size;

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
    memory_regions: Vec<MemoryRegion, MAX_TVM_MEMORY_REGIONS>,
    state_enum: TvmState,
    vcpu: Option<TvmVcpuState>,
    entry_sepc: usize,
    entry_arg: usize,
    tvm_identity_addr: usize,
    hasher: sha2::Sha384,
    measure: alloc::vec::Vec<u8>,
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
            measure: alloc::vec::Vec::new(),
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

    pub fn get_measure(&self) -> alloc::vec::Vec<u8> {
        self.measure.clone()
    }
}

#[derive(Clone)]
enum TvmState {
    TvmInitializing = 0,
    TvmRunnable = 1,
    TvmStopped = 2,
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

static mut PAGE_FAULT_COUNTER: usize = 0;

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
        .create_tvm(attestation, pt_addr, state_addr)?;

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
    state.hypervisor.finalize_tvm(tvm_id, entry_point, 0, 0)?;

    Ok(tvm_id)
}
