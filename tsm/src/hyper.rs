use common::{
    attestation::{DiceLayer, Evidence, TsmAttestationContext, TvmAttestationContext},
    sbi::PAGE_SIZE,
};
use heapless::Vec;
use riscv::register::{
    sstatus::{self, FS, SPP},
    stvec::{self, Stvec},
};
use sha2::{Digest, Sha384};

use crate::{
    constants::{
        MAX_HARTS, MAX_TVMS, MAX_TVM_MEMORY_REGIONS, MIN_PAGE_DIRECTORY_SIZE, TVM_MAX_VCPUS,
    },
    gpt::{map_region, MemoryRegion, PTE_A, PTE_D, PTE_R, PTE_W, PTE_X},
    h_extension::{
        csrs::{hedeleg, henvcfg, hgatp, hideleg, hstatus, vsatp},
        instruction::hfence_gvma_all,
    },
    trap::{hyper_trap, VmTrapContext},
};

// -----------------------------
// Core TSM structures
// -----------------------------

struct PhysicalMemory {
    base_address: usize,
    size: usize,
    guest_id: Option<usize>,
}

// Track ELF segments to know what to copy where during lazy loading.
pub struct LazySegment {
    pub gpa: usize,
    pub memsz: usize,
    pub filesz: usize,
    pub offset: usize,
}

pub struct LazyState {
    pub(crate) elf_data: &'static [u8],
    pub(crate) segments: alloc::vec::Vec<LazySegment>,
    pub(crate) dtb_gpa: usize,
    pub(crate) dtb_data: &'static [u8],
    pub(crate) initrd_gpa: usize,
    pub(crate) initrd_data: &'static [u8],
    pub(crate) next_free_phys: usize,
    pub(crate) phys_limit: usize,
}

impl LazyState {
    pub fn new(
        segments: alloc::vec::Vec<LazySegment>,
        elf_data: &'static [u8],
        dtb_gpa: usize,
        dtb_data: &'static [u8],
        initrd_gpa: usize,
        initrd_data: &'static [u8],
        next_free_phys: usize,
        phys_limit: usize,
    ) -> Self {
        Self {
            elf_data,
            segments,
            dtb_gpa,
            dtb_data,
            initrd_gpa,
            initrd_data,
            next_free_phys,
            phys_limit,
        }
    }
}

#[derive(Clone)]
pub struct VcpuRef {
    pub tvmid: usize,
    pub vcpuid: usize,
}

struct HartState {
    current: Option<VcpuRef>,
}

pub struct HypervisorState {
    /* Persisten TVM state */
    tvms: [Option<Tvm>; MAX_TVMS],

    /* Base page address, num pages, vmid */
    confidential_memory: Vec<PhysicalMemory, MAX_TVM_MEMORY_REGIONS>,

    /* Per physical-hart execution state */
    harts: [HartState; MAX_HARTS],
}

#[inline(always)]
// TODO(multi-hart): S-mode can't read mhartid; the firmware must pass the
// hartid in. Until then we are hart 0.
pub fn current_hartid() -> usize {
    0
}

impl HypervisorState {
    pub fn new() -> Self {
        Self {
            tvms: core::array::from_fn(|_| None),
            confidential_memory: Vec::new(),
            harts: core::array::from_fn(|_| HartState { current: None }),
        }
    }

    pub fn running_tvm(&self) -> Option<&Tvm> {
        let current = self.current_vcpu()?;
        self.tvms.get(current.tvmid).and_then(|tvm| tvm.as_ref())
    }

    pub fn running_tvm_mut(&mut self) -> Option<&mut Tvm> {
        let current = self.current_vcpu()?;
        self.tvms
            .get_mut(current.tvmid)
            .and_then(|tvm| tvm.as_mut())
    }

    pub fn tvm(&self, tvmid: usize) -> Option<&Tvm> {
        self.tvms.get(tvmid).and_then(|tvm| tvm.as_ref())
    }

    pub fn set_tvm_lazy_state(
        &mut self,
        tvmid: usize,
        lazy_state: LazyState,
    ) -> anyhow::Result<()> {
        match self.tvms.get_mut(tvmid).and_then(|tvm| tvm.as_mut()) {
            Some(tvm) if matches!(tvm.state_enum, TvmState::TvmInitializing) => {
                tvm.lazy_state = Some(lazy_state);
                Ok(())
            }
            Some(_) => Err(anyhow::anyhow!("cannot set lazy state after finalization")),
            None => Err(anyhow::anyhow!("no tvm present")),
        }
    }
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
        owner: usize,
        attestation_context: TvmAttestationContext,
        page_table_addr: usize,
        state_addr: usize,
    ) -> anyhow::Result<usize> {
        if page_table_addr % MIN_PAGE_DIRECTORY_SIZE != 0 {
            return Err(anyhow::anyhow!("page table addr must be 16KB-aligned"));
        }

        let page_table_size = state_addr
            .checked_sub(page_table_addr)
            .ok_or_else(|| anyhow::anyhow!("state address precedes page table"))?;

        if page_table_size < MIN_PAGE_DIRECTORY_SIZE || page_table_size % PAGE_SIZE != 0 {
            return Err(anyhow::anyhow!("invalid page table size"));
        }

        let tvmid = (0..MAX_TVMS)
            .find(|&i| self.tvms[i].is_none())
            .ok_or_else(|| anyhow::anyhow!("cannot create new TVM"))?;

        self.tvms[tvmid] = Some(Tvm::new(
            owner,
            attestation_context,
            page_table_addr,
            page_table_size,
            state_addr,
        ));

        Self::claim_confidential_range(
            &mut self.confidential_memory,
            page_table_addr,
            page_table_size,
            tvmid,
        )?;
        Self::claim_confidential_range(
            &mut self.confidential_memory,
            state_addr,
            PAGE_SIZE,
            tvmid,
        )?;

        unsafe {
            let ptr = page_table_addr as *mut u8;
            core::ptr::write_bytes(ptr, 0, page_table_size);
        }

        Ok(tvmid)
    }

    pub fn finalize_tvm(
        &mut self,
        tvmid: usize,
        entry_sepc: usize,
        entry_arg: usize,
        tvm_identity_addr: usize,
        tsm_context: &TsmAttestationContext,
    ) -> anyhow::Result<()> {
        match &mut self.tvms[tvmid] {
            Some(tvm) => {
                tvm.finalize(entry_sepc, entry_arg, tvm_identity_addr, tsm_context);
                Ok(())
            }
            None => Err(anyhow::anyhow!("no tvm present")),
        }
    }

    pub fn destroy_tvm(&mut self, tvmid: usize) -> anyhow::Result<()> {
        match &self.tvms[tvmid] {
            Some(tvm) => match tvm.state_enum {
                TvmState::TvmStopped => {
                    self.tvms[tvmid] = None;
                    for region in self.confidential_memory.iter_mut() {
                        if let Some(guest) = region.guest_id {
                            if guest == tvmid {
                                region.guest_id = None;
                            }
                        }
                    }
                    self.coalesce_confidential_blocks();
                    Ok(())
                }
                _ => Err(anyhow::anyhow!("tvm is not stopped")),
            },
            None => Err(anyhow::anyhow!("no tvm available")),
        }
    }

    pub fn add_tvm_memory_region(
        &mut self,
        tvmid: usize,
        tvm_gpa_addr: usize,
        region_len_bytes: usize,
    ) -> anyhow::Result<()> {
        match &mut self.tvms[tvmid] {
            Some(tvm) => {
                match tvm.state_enum {
                    TvmState::TvmInitializing => {}
                    _ => {
                        return Err(anyhow::anyhow!(
                            "cannot add memory region unless TVM_INITIALIZING"
                        ))
                    }
                }

                if (tvm_gpa_addr % PAGE_SIZE) != 0
                    || (region_len_bytes % PAGE_SIZE) != 0
                    || region_len_bytes == 0
                {
                    return Err(anyhow::anyhow!(
                        "tvm_gpa_addr and region_len must be 4KB-aligned and non-zero"
                    ));
                }

                let num_pages = region_len_bytes / PAGE_SIZE;
                let new_a = tvm_gpa_addr;
                let new_b = tvm_gpa_addr + region_len_bytes;

                for r in tvm.memory_regions.iter() {
                    let r_a = r.guest_gpa_base;
                    let r_b = r.guest_gpa_base + r.num_pages * PAGE_SIZE;
                    if !(new_b <= r_a || r_b <= new_a) {
                        return Err(anyhow::anyhow!("region overlap with existing region"));
                    }
                }

                tvm.memory_regions
                    .push(MemoryRegion {
                        guest_gpa_base: tvm_gpa_addr,
                        num_pages,
                    })
                    .map_err(|_| anyhow::anyhow!("cannot push confidential memory regions"))?;

                Ok(())
            }
            None => Err(anyhow::anyhow!("no tvm present")),
        }
    }

    pub fn add_tvm_mmio_region(
        &mut self,
        tvmid: usize,
        guest_gpa: usize,
        host_pa: usize,
        size: usize,
    ) -> anyhow::Result<()> {
        if guest_gpa % PAGE_SIZE != 0
            || host_pa % PAGE_SIZE != 0
            || size == 0
            || size % PAGE_SIZE != 0
        {
            return Err(anyhow::anyhow!(
                "MMIO addresses and size must be page-aligned"
            ));
        }

        match &mut self.tvms[tvmid] {
            Some(tvm) => {
                match tvm.state_enum {
                    TvmState::TvmInitializing => {}
                    _ => return Err(anyhow::anyhow!("cannot add MMIO after finalization")),
                }
                // Check that the MMIO GPA does not overlap guest RAM.
                let mmio_end = guest_gpa + size;

                for region in &tvm.memory_regions {
                    let region_start = region.guest_gpa_base;
                    let region_end = region_start + region.num_pages * PAGE_SIZE;

                    if guest_gpa < region_end && region_start < mmio_end {
                        return Err(anyhow::anyhow!("MMIO overlaps an existing guest region"));
                    }
                }

                map_region(
                    tvm.page_table_addr,
                    tvm.page_table_size,
                    guest_gpa,
                    host_pa,
                    size / PAGE_SIZE,
                    PTE_R | PTE_W | PTE_A | PTE_D,
                );

                Ok(())
            }
            None => Err(anyhow::anyhow!("invalid TVMID")),
        }
    }

    pub fn add_tvm_measured_pages(
        &mut self,
        tvmid: usize,
        source_addr: usize,
        dest_addr: usize,
        tsm_page_type: usize,
        num_pages: usize,
        tvm_guest_gpa: usize,
    ) -> anyhow::Result<()> {
        if num_pages == 0 {
            return Ok(());
        }
        assert_eq!(tsm_page_type, 0, "accepting 4k pages for now");

        if dest_addr % PAGE_SIZE != 0 || tvm_guest_gpa % PAGE_SIZE != 0 || num_pages == 0 {
            return Err(anyhow::anyhow!(
                "destination/GPA must be page-aligned and num_pages non-zero"
            ));
        }

        let bytes = num_pages
            .checked_mul(PAGE_SIZE)
            .ok_or_else(|| anyhow::anyhow!("page range overflow"))?;

        let gpa_end = tvm_guest_gpa
            .checked_add(bytes)
            .ok_or_else(|| anyhow::anyhow!("GPA range overflow"))?;

        // Validate the TVM and GPA range first.
        {
            let tvm = self
                .tvms
                .get(tvmid)
                .and_then(|tvm| tvm.as_ref())
                .ok_or_else(|| anyhow::anyhow!("invalid TVMID"))?;

            if !matches!(tvm.state_enum, TvmState::TvmInitializing) {
                return Err(anyhow::anyhow!(
                    "cannot add measured pages after finalization"
                ));
            }

            let in_tvm_region = tvm.memory_regions.iter().any(|region| {
                let region_end = region.guest_gpa_base + region.num_pages * PAGE_SIZE;

                tvm_guest_gpa >= region.guest_gpa_base && gpa_end <= region_end
            });

            if !in_tvm_region {
                return Err(anyhow::anyhow!(
                    "GPA range 0x{:x}-0x{:x} not within any memory region",
                    tvm_guest_gpa,
                    gpa_end
                ));
            }
        }

        // Claim the physical confidential memory for this TVM.
        Self::claim_confidential_block(&mut self.confidential_memory, dest_addr, tvmid)?;

        // Reborrow the TVM after the previous immutable borrow ended.
        let tvm = self
            .tvms
            .get_mut(tvmid)
            .and_then(|tvm| tvm.as_mut())
            .ok_or_else(|| anyhow::anyhow!("invalid TVMID"))?;

        // Copy and measure the source data.
        unsafe {
            let src_ptr = source_addr as *const u8;
            let dst_ptr = dest_addr as *mut u8;

            core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, bytes);

            let content = core::slice::from_raw_parts(src_ptr, bytes);
            tvm.extend_measure(content);
        }

        map_region(
            tvm.page_table_addr,
            tvm.page_table_size,
            tvm_guest_gpa,
            dest_addr,
            num_pages,
            PTE_R | PTE_W | PTE_X | PTE_A | PTE_D,
        );

        Ok(())
    }

    pub fn add_tvm_zero_pages(
        &mut self,
        tvmid: usize,
        base_page_address: usize,
        tsm_page_type: usize,
        num_pages: usize,
        tvm_base_page_address: usize,
    ) -> anyhow::Result<()> {
        assert_eq!(tsm_page_type, 0, "accepting 4k pages for now");
        if (base_page_address % PAGE_SIZE) != 0 || (tvm_base_page_address % PAGE_SIZE) != 0 {
            return Err(anyhow::anyhow!("all addresses must be page-aligned"));
        }
        let bytes = num_pages
            .checked_mul(PAGE_SIZE)
            .ok_or_else(|| anyhow::anyhow!("page range overflow"))?;
        Self::claim_confidential_range(
            &mut self.confidential_memory,
            base_page_address,
            bytes,
            tvmid,
        )?;

        match &mut self.tvms[tvmid] {
            Some(tvm) => {
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
                    return Err(anyhow::anyhow!(
                        "GPA range 0x{:x}-0x{:x} not within any memory region",
                        tvm_base_page_address,
                        gpa_end
                    ));
                }

                map_region(
                    tvm.page_table_addr,
                    tvm.page_table_size,
                    tvm_base_page_address,
                    base_page_address,
                    num_pages,
                    PTE_R | PTE_W | PTE_X,
                );
                Ok(())
            }
            None => Err(anyhow::anyhow!("invalid TVMID")),
        }
    }

    pub fn create_tvm_vcpu(
        &mut self,
        tvmid: usize,
        vcpuid: usize,
        _tvm_state_page_addr: usize,
    ) -> anyhow::Result<()> {
        match &mut self.tvms[tvmid] {
            Some(tvm) => {
                if vcpuid > TVM_MAX_VCPUS - 1 {
                    return Err(anyhow::anyhow!("invalid vCPU id"));
                }
                if tvm.vcpus[vcpuid].is_none() {
                    tvm.vcpus[vcpuid] = Some(TvmVcpuState::new(vcpuid));
                    return Ok(());
                }
                Err(anyhow::anyhow!("vCPU already exists"))
            }
            None => Err(anyhow::anyhow!("invalid TVMID")),
        }
    }

    pub fn prepare_tvm_vcpu(
        &mut self,
        tvmid: usize,
        vcpuid: usize,
    ) -> anyhow::Result<(usize, usize, usize, bool)> {
        if vcpuid > TVM_MAX_VCPUS - 1 {
            return Err(anyhow::anyhow!("invalid vCPU id"));
        }
        if self.current_vcpu().is_some() {
            return Err(anyhow::anyhow!("a vCPU is already running"));
        }

        let tvm = &self
            .tvms
            .get(tvmid)
            .and_then(|tvm| tvm.as_ref())
            .ok_or_else(|| anyhow::anyhow!("invalid TVMID"))?;

        if !matches!(
            tvm.state_enum,
            TvmState::TvmRunnable | TvmState::TvmSuspended
        ) {
            return Err(anyhow::anyhow!("TVM must be runnable or suspended"));
        }

        let vcpu = tvm.vcpus[vcpuid]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no vcpu present"))?;
        let resume = vcpu.started;
        let vcpu_addr = vcpu as *const TvmVcpuState as usize;
        let entry_sepc = tvm.entry_sepc;
        let entry_arg = tvm.entry_arg;

        // Setup H-extension for guest execution.
        self.setup_h_extension(tvm)?;

        let tvm = self
            .tvms
            .get_mut(tvmid)
            .and_then(|tvm| tvm.as_mut())
            .ok_or_else(|| anyhow::anyhow!("invalid TVMID"))?;
        tvm.state_enum = TvmState::TvmRunnable;
        tvm.vcpus[vcpuid]
            .as_mut()
            .expect("vCPU disappeared while preparing")
            .started = true;

        self.harts[current_hartid()].current = Some(VcpuRef { tvmid, vcpuid });

        Ok((vcpu_addr, entry_sepc, entry_arg, resume))
    }

    pub fn current_vcpu(&self) -> Option<VcpuRef> {
        self.harts[current_hartid()].current.clone()
    }

    pub unsafe fn enter_prepared_tvm_vcpu(
        vcpu_addr: usize,
        entry_sepc: usize,
        entry_arg: usize,
        resume: bool,
    ) -> ! {
        (*(vcpu_addr as *const TvmVcpuState)).enter(entry_sepc, entry_arg, resume)
    }

    pub fn suspend_tvm(&mut self, tvmid: usize) -> anyhow::Result<usize> {
        let current = self
            .current_vcpu()
            .ok_or_else(|| anyhow::anyhow!("no running vCPU"))?;
        if current.tvmid != tvmid {
            return Err(anyhow::anyhow!("TVM is not running on this hart"));
        }

        let tvm = self
            .tvms
            .get_mut(tvmid)
            .and_then(|tvm| tvm.as_mut())
            .ok_or_else(|| anyhow::anyhow!("invalid TVMID"))?;
        if !matches!(tvm.state_enum, TvmState::TvmRunnable) {
            return Err(anyhow::anyhow!("TVM is not runnable"));
        }
        tvm.state_enum = TvmState::TvmSuspended;
        let owner = tvm.owner;
        self.harts[current_hartid()].current = None;

        Ok(owner)
    }

    pub fn tvm_shutdown(&mut self, tvmid: usize) -> anyhow::Result<usize> {
        match &mut self.tvms[tvmid] {
            Some(tvm) => match tvm.state_enum {
                TvmState::TvmRunnable => {
                    tvm.state_enum = TvmState::TvmStopped;
                    self.harts[current_hartid()].current = None;
                    Ok(tvm.owner)
                }
                _ => Err(anyhow::anyhow!("TVM is not running")),
            },
            None => Err(anyhow::anyhow!("invalid TVMID")),
        }
    }

    pub fn reclaim_pages(
        &mut self,
        base_page_address: usize,
        num_pages: usize,
    ) -> anyhow::Result<()> {
        let idx = self
            .confidential_memory
            .iter()
            .position(|pm| {
                pm.base_address == base_page_address
                    && (pm.size / PAGE_SIZE) == num_pages
                    && pm.guest_id.is_none()
            })
            .ok_or_else(|| anyhow::anyhow!("No matching memory block"))?;

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

    fn claim_confidential_block(
        blocks: &mut Vec<PhysicalMemory, MAX_TVM_MEMORY_REGIONS>,
        address: usize,
        tvmid: usize,
    ) -> anyhow::Result<()> {
        let block = blocks
            .iter_mut()
            .find(|block| {
                address >= block.base_address
                    && address < block.base_address.saturating_add(block.size)
            })
            .ok_or_else(|| anyhow::anyhow!("range not in confidential memory"))?;

        match block.guest_id {
            None => block.guest_id = Some(tvmid),
            Some(owner) if owner == tvmid => {}
            Some(_) => {
                return Err(anyhow::anyhow!(
                    "confidential memory already owned by another TVM"
                ))
            }
        }

        Ok(())
    }

    fn claim_confidential_range(
        blocks: &mut Vec<PhysicalMemory, MAX_TVM_MEMORY_REGIONS>,
        address: usize,
        size: usize,
        tvmid: usize,
    ) -> anyhow::Result<()> {
        let end = address
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("confidential range overflow"))?;

        let mut updated = Vec::new();
        let mut claimed = false;

        for block in blocks.iter() {
            let block_end = block.base_address.saturating_add(block.size);
            if address < block.base_address || end > block_end {
                updated
                    .push(PhysicalMemory {
                        base_address: block.base_address,
                        size: block.size,
                        guest_id: block.guest_id,
                    })
                    .map_err(|_| anyhow::anyhow!("too many confidential memory regions"))?;
                continue;
            }

            claimed = true;
            if let Some(owner) = block.guest_id {
                if owner != tvmid {
                    return Err(anyhow::anyhow!(
                        "confidential memory already owned by another TVM"
                    ));
                }

                updated
                    .push(PhysicalMemory {
                        base_address: block.base_address,
                        size: block.size,
                        guest_id: block.guest_id,
                    })
                    .map_err(|_| anyhow::anyhow!("too many confidential memory regions"))?;
                continue;
            }

            if block.base_address < address {
                updated
                    .push(PhysicalMemory {
                        base_address: block.base_address,
                        size: address - block.base_address,
                        guest_id: None,
                    })
                    .map_err(|_| anyhow::anyhow!("too many confidential memory regions"))?;
            }
            updated
                .push(PhysicalMemory {
                    base_address: address,
                    size,
                    guest_id: Some(tvmid),
                })
                .map_err(|_| anyhow::anyhow!("too many confidential memory regions"))?;
            if end < block_end {
                updated
                    .push(PhysicalMemory {
                        base_address: end,
                        size: block_end - end,
                        guest_id: None,
                    })
                    .map_err(|_| anyhow::anyhow!("too many confidential memory regions"))?;
            }
        }

        if !claimed {
            return Err(anyhow::anyhow!("range not in confidential memory"));
        }

        *blocks = updated;
        Ok(())
    }

    fn coalesce_confidential_blocks(&mut self) {
        let mut merged: Vec<PhysicalMemory, MAX_TVM_MEMORY_REGIONS> = Vec::new();

        for block in self.confidential_memory.iter() {
            if let Some(previous) = merged.last_mut() {
                let previous_end = previous.base_address + previous.size;
                if previous_end == block.base_address && previous.guest_id == block.guest_id {
                    previous.size += block.size;
                    continue;
                }
            }

            if merged
                .push(PhysicalMemory {
                    base_address: block.base_address,
                    size: block.size,
                    guest_id: block.guest_id,
                })
                .is_err()
            {
                panic!("too many coalesced confidential memory regions");
            }
        }

        self.confidential_memory = merged;
    }
}

#[repr(C)]
pub struct Tvm {
    owner: usize,
    page_table_addr: usize,
    page_table_size: usize,
    state_addr: usize,
    memory_regions: Vec<MemoryRegion, MAX_TVM_MEMORY_REGIONS>,
    state_enum: TvmState,
    vcpus: [Option<TvmVcpuState>; TVM_MAX_VCPUS],
    entry_sepc: usize,
    entry_arg: usize,
    tvm_identity_addr: usize,
    hasher: sha2::Sha384,
    measure: alloc::vec::Vec<u8>,
    attestation_context: TvmAttestationContext,
    lazy_state: Option<LazyState>,
}

impl Tvm {
    fn new(
        owner: usize,
        attestation_context: TvmAttestationContext,
        page_table_addr: usize,
        page_table_size: usize,
        state_addr: usize,
    ) -> Self {
        Self {
            owner,
            page_table_addr,
            page_table_size,
            state_addr,
            memory_regions: Vec::new(),
            state_enum: TvmState::TvmInitializing,
            vcpus: core::array::from_fn(|_| None),
            entry_sepc: 0,
            entry_arg: 0,
            tvm_identity_addr: 0,
            hasher: Sha384::new(),
            measure: alloc::vec::Vec::new(),
            attestation_context,
            lazy_state: None,
        }
    }
    pub fn get_evidence(&self, challenge: &[u8]) -> Evidence {
        self.attestation_context
            .get_evidence(&self.measure, challenge)
    }

    pub(crate) fn contains_gpa_range(&self, start: usize, end: usize) -> bool {
        self.memory_regions.iter().any(|region| {
            let region_end = region.guest_gpa_base + region.num_pages * PAGE_SIZE;
            start >= region.guest_gpa_base && end <= region_end
        })
    }

    pub(crate) fn page_table(&self) -> (usize, usize) {
        (self.page_table_addr, self.page_table_size)
    }

    pub(crate) fn lazy_state_mut(&mut self) -> Option<&mut LazyState> {
        self.lazy_state.as_mut()
    }

    fn finalize(
        &mut self,
        entry_sepc: usize,
        entry_arg: usize,
        tvm_identity_addr: usize,
        tsm_context: &TsmAttestationContext,
    ) {
        self.entry_sepc = entry_sepc;
        self.entry_arg = entry_arg;
        self.tvm_identity_addr = tvm_identity_addr;
        self.state_enum = TvmState::TvmRunnable;

        let old_hasher = core::mem::take(&mut self.hasher);
        self.measure = old_hasher.finalize().to_vec();
        self.hasher = Sha384::new();

        self.attestation_context = tsm_context.compute_next(&self.measure);
    }

    fn extend_measure(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }
}

#[derive(Clone)]
enum TvmState {
    TvmInitializing = 0,
    TvmRunnable = 1,
    TvmStopped = 2,
    TvmSuspended = 3,
}

#[repr(C, align(4))]
struct TvmVcpuState {
    id: usize,
    started: bool,
    trap_ctx: VmTrapContext,
    // Hypervisor scratch stack (grows downward from end)
    hs_scratch_stack: [u8; 1024 * 128],
}

impl TvmVcpuState {
    fn new(id: usize) -> Self {
        let vcpu = Self {
            id,
            started: false,
            trap_ctx: VmTrapContext {
                regs: [0; 32],
                hs_sp: 0,
                sepc: 0,
                sstatus: 0,
            },
            hs_scratch_stack: [0; 1024 * 128],
        };
        vcpu
    }

    unsafe fn enter(&self, entry_sepc: usize, entry_arg: usize, resume: bool) -> ! {
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

        if !resume {
            (*trap_ctx_mut).regs = [0; 32];
            (*trap_ctx_mut).regs[10] = self.id;
            (*trap_ctx_mut).regs[11] = entry_arg;
            (*trap_ctx_mut).sepc = entry_sepc;
            (*trap_ctx_mut).sstatus = sstatus::read().bits();
        }

        core::arch::asm!(
            "ld x1,   8(t6)
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
             ld t0, 264(t6)
             csrw sepc, t0
             ld t0, 272(t6)
             csrw sstatus, t0
             ld x5, 40(t6)
             ld t6, 248(t6)
             fence.i
             sret",
            in("t6") ctx,
            options(noreturn, nostack),
        )
    }
}
