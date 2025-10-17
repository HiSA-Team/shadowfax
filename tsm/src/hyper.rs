use heapless::Vec;
use riscv::register::{
    satp, sepc,
    sstatus::{self, FS},
    stvec::{self, Stvec},
};

use crate::{
    guest_page_table::GuestPageTable,
    h_extension::{
        csrs::{
            hedeleg::{self, ExceptionKind},
            hgatp, hideleg, hstatus, hvip, vsatp, VsInterruptKind,
        },
        instruction::hfence_gvma_all,
    },
};

const MAX_VCPU_PER_TVM: usize = 1;
const MAX_PAGE_BLOCKS: usize = 4;
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

/// Minimal SV39 mapper for one 4 KiB page.
/// Uses the 16 KiB region:
///   L2=root, L1=root+4K, L0=root+8K.
fn map_4k_leaf(root_pt: usize, gpa: usize, pa: usize, perms: u64) {
    assert_eq!(gpa % PAGE_SIZE, 0);
    assert_eq!(pa % PAGE_SIZE, 0);

    let [vpn2, vpn1, vpn0] = make_vpn_sv39(gpa);

    // Level 2 → Level 1
    let pte2_addr = root_pt + vpn2 * PTE_SIZE;
    let l1_base = root_pt + 0x1000;
    let pte = (pa_to_ppn(l1_base) << 10) | PTE_V;
    unsafe {
        core::ptr::write_volatile(pte2_addr as *mut u64, pte);
    }

    // Level 1 → Level 0
    let pte1_addr = l1_base + vpn1 * PTE_SIZE;
    let l0_base = root_pt + 0x2000;
    let pte = (pa_to_ppn(l0_base) << 10) | PTE_V;
    unsafe {
        core::ptr::write_volatile(pte1_addr as *mut u64, pte);
    }

    // Level 0 (leaf)
    let pte0_addr = l0_base + vpn0 * PTE_SIZE;
    let leaf = (pa_to_ppn(pa) << 10) | perms | PTE_V | PTE_U;
    unsafe {
        core::ptr::write_volatile(pte0_addr as *mut u64, leaf);
    }
}

// -----------------------------
// Core TSM structures
// -----------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub guest_gpa_base: usize,
    pub num_pages: usize,
}

#[repr(C)]
pub struct HypervisorState {
    pub tvm: Option<Tvm>,
    confidential_memory: Vec<(usize, usize, Option<usize>), MAX_PAGE_BLOCKS>,
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
            .push((base_page_addr, num_pages, None))
            .map_err(|c| {
                anyhow::anyhow!(
                    "out of confidential memory; failed to insert {} pages from {}",
                    c.1,
                    c.0
                )
            })?;
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

        // init the page table
        let table = GuestPageTable::new();
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

        t.memory_regions
            .push(MemoryRegion {
                guest_gpa_base: tvm_gpa_addr,
                num_pages,
            })
            .map_err(|_| anyhow::anyhow!("too many memory regions"))?;
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

        let t = self.tvm.as_mut().unwrap();
        if t.id != tvm_id {
            anyhow::bail!("tvm id mismatch");
        }

        match t.state_enum {
            TvmState::TvmInitializing => {}
            _ => anyhow::bail!("cannot alter TVM unless TVM_INITIALIZING"),
        }

        assert_eq!(tsm_page_type, 0, "accepting 4k pages for now");

        unsafe {
            let tot_bytes = PAGE_SIZE * num_pages;
            core::ptr::copy_nonoverlapping(
                source_addr as *const u8,
                dest_addr as *mut u8,
                tot_bytes,
            );
            // map GPA → PA in the TVM's page table
            map_4k_leaf(
                t.page_table_addr,
                tvm_guest_gpa,
                dest_addr,
                PTE_R | PTE_X | PTE_U,
            );
        }

        Ok(())
    }

    pub fn tvm_run_vcpu(&self, tvm_id: usize, tvm_vcpu_id: usize) -> anyhow::Result<!> {
        if let Some(tvm) = &self.tvm {
            if tvm.id != tvm_id {
                return Err(anyhow::Error::msg("no VM"));
            }
            tvm.run_vcpu(tvm_vcpu_id)?
        }

        anyhow::bail!("no VM")
    }

    fn find_confidential_block_idx_covering(&self, addr: usize, size: usize) -> Option<usize> {
        for (i, &(base, npages, _owner)) in self.confidential_memory.iter().enumerate() {
            let region_bytes = npages * PAGE_SIZE;
            if addr >= base && (addr + size) <= (base + region_bytes) {
                return Some(i);
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
    pub memory_regions: Vec<MemoryRegion, MAX_MEMORY_REGIONS>,
    pub state_enum: TvmState,
    pub vcpus: Vec<TvmVcpuState, MAX_VCPU_PER_TVM>,
    pub entry_sepc: usize,
    pub entry_arg: usize,
    pub tvm_identity_addr: usize,
}

impl Tvm {
    fn new(page_table_addr: usize, state_addr: usize) -> Self {
        Self {
            id: 1,
            page_table_addr,
            state_addr,
            memory_regions: Vec::new(),
            state_enum: TvmState::TvmInitializing,
            vcpus: Vec::new(),
            entry_sepc: 0,
            entry_arg: 0,
            tvm_identity_addr: 0,
        }
    }

    fn finalize(&mut self, entry_sepc: usize, entry_arg: usize, tvm_identity_addr: usize) {
        self.entry_sepc = entry_sepc;
        self.entry_arg = entry_arg;
        self.tvm_identity_addr = tvm_identity_addr;
        self.state_enum = TvmState::TvmRunnable;
    }

    /// This function actually performs the context switch into VS-mode starting the guest
    fn run_vcpu(&self, vcpu_id: usize) -> anyhow::Result<!> {
        let vcpu = self
            .vcpus
            .iter()
            .enumerate()
            .find(|(id, _)| *id == vcpu_id)
            .ok_or_else(|| anyhow::anyhow!("no vcpu"))?;

        hvip::clear(VsInterruptKind::External);
        hvip::clear(VsInterruptKind::Timer);
        hvip::clear(VsInterruptKind::Software);

        // disable address translation.
        vsatp::write(0);

        // specify delegation exception kinds.
        hedeleg::write(
            ExceptionKind::InstructionAddressMissaligned as usize
                | ExceptionKind::Breakpoint as usize
                | ExceptionKind::EnvCallFromUorVU as usize
                | ExceptionKind::InstructionPageFault as usize
                | ExceptionKind::LoadPageFault as usize
                | ExceptionKind::StoreAmoPageFault as usize,
        );

        // specify delegation interrupt kinds.
        hideleg::write(
            VsInterruptKind::External as usize
                | VsInterruptKind::Timer as usize
                | VsInterruptKind::Software as usize,
        );

        // configure the root page table address, VMID and specify SV39 pagination mode
        // finally issue the hfence_gvma_all to flush the TLB
        hgatp::set(hgatp::Mode::Sv39x4, self.id, self.page_table_addr >> 12);
        hfence_gvma_all();

        unsafe {
            // sstatus.SUM = 1, sstatus.SPP = 0
            sstatus::set_sum();
            sstatus::set_spp(sstatus::SPP::Supervisor);
            // sstatus.sie = 1
            sstatus::set_sie();

            // hstatus.spv = 1 (enable V bit when sret executed)
            hstatus::set_spv();

            // set entry point
            sepc::write(self.entry_sepc);

            core::arch::asm!(
                r#"
                    fence.i
                    sret
                "#,

                in("a0") vcpu.0,
                in("a1") 0,
                options(nomem, noreturn, nostack)
            )
        }
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

    sstatus: usize,
    stvec: usize,
    sip: usize,
    satp: usize,
    sepc: usize,
}
