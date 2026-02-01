use alloc::vec::Vec;
use common::attestation::{DiceLayer, TvmAttestationContext};
use core::alloc::Layout;
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

use crate::{
    h_extension::{
        csrs::{hgatp, hstatus, htval, vsatp},
        instruction::hfence_gvma_all,
        HvException,
    },
    println, TsmState,
};

const PAGE_SIZE: usize = 4096;
const PAGE_DIRECTORY_SIZE: usize = 16 * 1024;

const PTE_SIZE: usize = 8;
const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;

// -----------------------------
// SBI Helper
// -----------------------------
struct SbiRet {
    error: usize,
    value: usize,
}

fn sbi_call(eid: usize, fid: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> SbiRet {
    let (error, value);
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") eid,
            in("a6") fid,
            inout("a0") a0 => error,
            inout("a1") a1 => value,
            in("a2") a2,
            in("a3") a3,
        );
    }
    SbiRet { error, value }
}

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
    // assert_eq!(gpa % PAGE_SIZE, 0, "GPA must be page-aligned");
    // assert_eq!(pa % PAGE_SIZE, 0, "PA must be page-aligned");

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
        // TODO align GPA to PAGE
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
    tvm: Option<Tvm>,
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

        let tvm = Tvm::new(attestation_context, page_table_addr, state_addr);
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

    /// Setup H-extension CSRs for guest execution
    fn setup_h_extension(&self, tvm: &Tvm) -> anyhow::Result<()> {
        // Disable VS-mode address translation (guest manages its own)
        vsatp::write(0);

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
struct Tvm {
    id: usize,
    page_table_addr: usize,
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
        state_addr: usize,
    ) -> Self {
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
    }

    fn extend_measure(&mut self, data: &[u8]) {
        self.hasher.update(data);
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
    hs_scratch_stack: [u8; 4096],
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
            hs_scratch_stack: [0; 4096],
        };
        // We write vhartid in a0
        vcpu.regs[10] = id;
        vcpu
    }

    unsafe fn enter(&self, entry_sepc: usize, _entry_arg: usize) -> ! {
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
        // Restore Guest t6 and set up sscratch for next trap
        "ld t0, 248(t6)",    // Load saved Guest t6 into t0
        "csrw sscratch, t6", // Put VmTrapContext pointer back into sscratch
        "mv t6, t0",         // Finally restore Guest t6
        "sret",
    )
}

#[no_mangle]

extern "C" fn hyper_trap_handler_rust(ctx: *mut VmTrapContext) -> *mut VmTrapContext {
    let scause = riscv::register::scause::read();
    let stval = riscv::register::stval::read(); // GPA on Guest Page Fault
    let mut sepc = riscv::register::sepc::read();

    match scause.cause() {
        Trap::Interrupt(interrupt_number) => {
            panic!("Interrupt {} not handled", interrupt_number);
        }

        Trap::Exception(exception_number) => match exception_number {
            _ => match HvException::from(scause.code()) {
                HvException::EcallFromVsMode => {
                    let regs = unsafe { &mut (*ctx).regs };

                    // 1. Forward to OpenSBI (M-mode)

                    // eid = a7 (regs[17]), fid = a6 (regs[16])

                    let sbi_ret =
                        sbi_call(regs[17], regs[16], regs[10], regs[11], regs[12], regs[13]);

                    // 2. Write return values back to Guest a0, a1

                    regs[10] = sbi_ret.error;

                    regs[11] = sbi_ret.value;

                    // 3. Skip the 'ecall' instruction in the guest

                    sepc += 4;

                    unsafe {
                        riscv::register::sepc::write(sepc);
                    }
                }

                HvException::InstructionGuestPageFault => {
                    panic!(
                        "Instruction guest-page fault\nfault gpa: {:#x}\nfault hpa: {:#x}",
                        stval,
                        htval::read().bits() << 2,
                    );
                }

                HvException::LoadGuestPageFault => {}
                HvException::StoreAmoGuestPageFault => {}
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

pub fn bootstrap_load_elf(
    state: &mut TsmState,
    data: &[u8],
    pt_addr: usize,
    state_addr: usize,
    conf_pool_base: usize,
) -> anyhow::Result<usize> {
    let elf = ElfBytes::<AnyEndian>::minimal_parse(data)
        .map_err(|e| anyhow::anyhow!("ELF parse error: {:?}", e))?;

    // 1. Create TVM
    let attestation = state.attestation_context.compute_next(&[0; 32]);
    let tvm_id = state
        .hypervisor
        .create_tvm(attestation, pt_addr, state_addr)?;

    // 2. Define Guest RAM - MATCH LINKER SCRIPT (ORIGIN = 0x1000)
    let gpa_base = 0x1000;
    let ram_size = 2 * 1024 * 1024; // 2MB
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
        let p_vaddr = ph.p_vaddr as usize;
        let p_filesz = ph.p_filesz as usize;
        let p_memsz = ph.p_memsz as usize;
        let p_offset = ph.p_offset as usize;

        // Alignment Math
        let gpa_page_start = p_vaddr & !(PAGE_SIZE - 1);
        let offset_in_page = p_vaddr - gpa_page_start;
        let num_measured_pages = (offset_in_page + p_filesz + PAGE_SIZE - 1) / PAGE_SIZE;

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
