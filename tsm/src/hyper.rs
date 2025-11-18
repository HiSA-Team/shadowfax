use core::slice::from_raw_parts;

use alloc::vec::Vec;
use riscv::register::{
    sepc,
    sstatus::{self, FS, SPP},
};
use sha2::{Digest, Sha384};

use crate::h_extension::{
    csrs::{
        hedeleg::{self, ExceptionKind},
        hgatp, hideleg, hstatus, hvip, vsatp, VsInterruptKind,
    },
    instruction::hfence_gvma_all,
};

const MAX_VCPU_PER_TVM: usize = 1;
const PAGE_SIZE: usize = 4096;
const PAGE_DIRECTORY_SIZE: usize = 16 * 1024;
const MAX_MEMORY_REGIONS: usize = 8; // per-TVM simple limit

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
/// Dynamically allocates page tables within the 16KB region as needed.
///
/// Memory layout:
///   root_pt + 0x0000: L2 table (root)
///   root_pt + 0x1000: L1 table (shared for all VPN[2]=0)
///   root_pt + 0x2000: First L0 table
///   root_pt + 0x3000: Second L0 table (if needed for different VPN[1])
///
/// Note: This assumes all mappings use VPN[2]=0 (addresses < 1GB)
fn map_4k_leaf(root_pt: usize, gpa: usize, pa: usize, perms: u64) {
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
        // For simplicity: L0 for VPN[1]=0 at root+0x2000, VPN[1]=1 at root+0x3000
        let l0_base = root_pt + 0x2000 + (vpn1 * PAGE_SIZE);

        // Check we don't exceed our 16KB region
        assert!(
            l0_base + PAGE_SIZE <= root_pt + PAGE_DIRECTORY_SIZE,
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

/// Map a contiguous region of memory (multiple 4KB pages).
fn map_region(root_pt: usize, gpa_base: usize, pa_base: usize, num_pages: usize, perms: u64) {
    for i in 0..num_pages {
        let gpa = gpa_base + i * PAGE_SIZE;
        let pa = pa_base + i * PAGE_SIZE;
        map_4k_leaf(root_pt, gpa, pa, perms);
    }
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
    confidential_memory: Vec<(usize, usize, Option<usize>)>,
}

impl HypervisorState {
    pub fn new() -> Self {
        Self {
            tvm: None,
            confidential_memory: Vec::new(),
        }
    }
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
        page_table_addr: usize,
        state_addr: usize,
    ) -> anyhow::Result<usize> {
        if self.tvm.is_some() {
            anyhow::bail!("already created tvm");
        }

        if page_table_addr % PAGE_DIRECTORY_SIZE != 0 {
            anyhow::bail!("page table addr must be 16KB-aligned");
        }

        assert!(
            state_addr < page_table_addr
                || state_addr > (page_table_addr + PAGE_DIRECTORY_SIZE - 8)
        );

        let pd_block_idx = self
            .find_confidential_block_idx_covering(page_table_addr, PAGE_DIRECTORY_SIZE)
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
            core::ptr::write_bytes(ptr, 0, PAGE_DIRECTORY_SIZE);
        }

        let tvm = Tvm::new(page_table_addr, state_addr);
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
                core::ptr::write_bytes(ptr, 0, PAGE_DIRECTORY_SIZE);
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

        if (source_addr % PAGE_SIZE) != 0
            || (dest_addr % PAGE_SIZE) != 0
            || (tvm_guest_gpa % PAGE_SIZE) != 0
        {
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

            let content = from_raw_parts(src_ptr, bytes);
            tvm.extend_measure(content);
        }

        // Map each page in the TVM's page table
        map_region(
            tvm.page_table_addr,
            tvm_guest_gpa,
            dest_addr,
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

    pub fn run_tvm_vcpu(&self, tvm_id: usize, vcpu_id: usize) -> anyhow::Result<!> {
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

        if vcpu.get_id() != vcpu_id {
            anyhow::bail!("invalid vcpu id");
        }

        match tvm.state_enum {
            TvmState::TvmRunnable => {}
            _ => anyhow::bail!("TVM must be in runnable state"),
        }

        // Setup H-extension for guest execution
        self.setup_h_extension(&tvm)?;

        unsafe { vcpu.enter(tvm.entry_sepc, tvm.entry_arg) }
    }

    /// Setup H-extension CSRs for guest execution
    fn setup_h_extension(&self, tvm: &Tvm) -> anyhow::Result<()> {
        // Clear pending interrupts
        hvip::clear(VsInterruptKind::External);
        hvip::clear(VsInterruptKind::Timer);
        hvip::clear(VsInterruptKind::Software);

        // Disable VS-mode address translation (guest manages its own)
        vsatp::write(0);

        // Delegate exceptions to VS-mode
        hedeleg::write(
            ExceptionKind::InstructionAddressMissaligned as usize
                | ExceptionKind::Breakpoint as usize
                | ExceptionKind::EnvCallFromUorVU as usize
                | ExceptionKind::InstructionPageFault as usize
                | ExceptionKind::LoadPageFault as usize
                | ExceptionKind::StoreAmoPageFault as usize,
        );

        // Delegate interrupts to VS-mode
        hideleg::write(
            VsInterruptKind::External as usize
                | VsInterruptKind::Timer as usize
                | VsInterruptKind::Software as usize,
        );

        // Setup guest physical address translation (G-stage)
        hgatp::set(hgatp::Mode::Sv39x4, 0, tvm.page_table_addr >> 12);

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
    pub id: usize,
    pub page_table_addr: usize,
    pub state_addr: usize,
    pub memory_regions: Vec<MemoryRegion>,
    pub state_enum: TvmState,
    pub vcpu: Option<TvmVcpuState>,
    pub entry_sepc: usize,
    pub entry_arg: usize,
    pub tvm_identity_addr: usize,
    pub hasher: sha2::Sha384,
    pub measure: Vec<u8>,
}

impl Tvm {
    fn new(page_table_addr: usize, state_addr: usize) -> Self {
        Self {
            id: 1,
            page_table_addr,
            state_addr,
            memory_regions: Vec::new(),
            state_enum: TvmState::TvmInitializing,
            vcpu: None,
            entry_sepc: 0,
            entry_arg: 0,
            tvm_identity_addr: 0,
            hasher: Sha384::new(),
            measure: Vec::new(),
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
    }

    fn extend_measure(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }
}

#[derive(Clone)]
pub enum TvmState {
    TvmInitializing = 0,
    TvmRunnable = 1,
}

#[derive(Clone, Debug)]
#[repr(C, align(4))]
pub struct TvmVcpuState {
    pub regs: [usize; 32],
    pub sstatus: usize,
    pub stvec: usize,
    pub sip: usize,
    pub satp: usize,
    pub sepc: usize,
    pub scause: usize,
    pub stval: usize,
}

impl TvmVcpuState {
    pub fn new(id: usize) -> Self {
        let mut vcpu = Self {
            regs: [0; 32],
            sstatus: 0,
            stvec: 0,
            sip: 0,
            satp: 0,
            sepc: 0,
            scause: 0,
            stval: 0,
        };

        // We write vhartid in a0
        vcpu.regs[10] = id;

        vcpu
    }

    pub fn init(&mut self, sepc: usize, arg: usize) {
        self.stvec = sepc;
        self.sepc = sepc;
        // a1 reg
        self.regs[11] = arg;
    }

    pub fn get_id(&self) -> usize {
        self.regs[10]
    }

    pub unsafe fn enter(&self, entry_sepc: usize, entry_arg: usize) -> ! {
        sstatus::set_sum(); // Allow supervisor to access user pages
        sstatus::set_spp(SPP::Supervisor); // Return to S-mode (VS-mode with SPV=1)
        sstatus::set_sie(); // Enable interrupts
        sstatus::set_fs(FS::Initial); // Enable FP state

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
            options(readonly, noreturn, nostack)
        )
    }
}
