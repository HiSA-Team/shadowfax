#![no_std]
#![no_main]
#![feature(fn_align)]

extern crate alloc;

use alloc::vec::Vec;

use crate::sbi::{
    cove::{TsmInfo, COVH_EXT_ID, MAX_DOMAINS, SBI_EXT_COVH_GET_TSM_INFO},
    sbi_call,
};
use core::{arch::asm, cell::OnceCell};
use h_extension::csrs::{
    hedeleg::{self, ExceptionKind},
    hgatp, hideleg, hstatus, hvip, vsatp, VsInterruptKind,
};
use riscv::register::{
    sepc,
    sstatus::{self, FS},
};
use spin::Mutex;

use crate::sbi::cove::{SBI_EXT_SUPD_GET_ACTIVE_DOMAINS, SUPD_EXT_ID};

use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

mod h_extension;
mod log;

#[link_section = ".guest_kernel"]
#[used]
static GUEST_KERNEL: [u8; include_bytes!("../empty.elf").len()] = *include_bytes!("../empty.elf");

unsafe extern "C" {
    /// boot stack top (defined in `memory.x`)
    static _top_b_stack: u8;
    static mut _hv_heap_start: u8;
    static _hv_heap_end: u8;
    static _start_bss: u8;
    static _end_bss: u8;
}

macro_rules! cove_pack_fid {
    ($sdid:expr, $fid:expr) => {
        (($sdid & 0x3F) << 26) | ($fid & 0xFFFF)
    };
}

#[derive(Debug)]
pub struct DomainInfo {
    pub domain_id: usize,
    pub tsm_info: TsmInfo,
}

const MAX_NUM_GUESTS: usize = 8;

#[repr(C)]
struct GuestContext {
    pub regs: [u64; 32],
    pub sstatus: usize,
    pub sepc: usize,
}

struct Guest {
    pub entry_point: usize,

    // stack pointer
    pub stack_pointer: usize,

    // points to context
    pub context_addr: usize,
}

// Global hypervisor data structure
struct HState {
    guests: heapless::Vec<Guest, MAX_NUM_GUESTS>,
}

impl HState {
    fn new() -> Self {
        Self {
            guests: heapless::Vec::new(),
        }
    }
}

static H_STATE: Mutex<OnceCell<HState>> = Mutex::new(OnceCell::new());

// Give each hart 4K stack
const STACK_SIZE_PER_HART: usize = 1024 * 4;

#[link_section = ".text.entry"]
#[no_mangle]
extern "C" fn entry() -> ! {
    unsafe {
        core::arch::asm!(
            // setup up the stack
            "li t0, {stack_size_per_hart}",
            "mul t1, a0, t0",
            "la sp, {stack_top}",
            "sub sp, sp, t1",

            "call {main}",

            stack_size_per_hart = const STACK_SIZE_PER_HART,
            stack_top = sym _top_b_stack,
            main = sym main,
            options(noreturn)
        )
    }
}

/// Main function.
/// -  Heap setup
fn main(_hartid: usize, _fdt_address: usize) -> ! {
    println!("[SHADOWFAX-HYPERVISOR] initializing");
    // clear bss section
    unsafe {
        use crate::{_end_bss, _start_bss};
        use core::ptr::addr_of;

        core::slice::from_raw_parts_mut(
            addr_of!(_start_bss).cast_mut(),
            addr_of!(_end_bss) as usize - addr_of!(_start_bss) as usize,
        )
        .fill(0);
    }

    unsafe {
        // Initialize global allocator
        let heap_start = &raw const _hv_heap_start as *const u8 as usize;
        let heap_end = &raw const _hv_heap_end as *const u8 as usize;
        let heap_size = heap_end - heap_start;
        println!(
            "[SHADOWFAX-HYPERVISOR] heap_start=0x{:x}, heap_end=0x{:x}, heap_size=0x{:x}",
            heap_start, heap_end, heap_size
        );
        ALLOCATOR
            .lock()
            .init(&raw mut _hv_heap_start as *mut u8, heap_size);
    }

    // Discover and query all active domains in one go
    match discover_and_query_domains() {
        Ok(domains) => {
            for domain in &domains {
                println!(
                    "[SHADOWFAX-HYPERVISOR] Domain {} - TSM impl_id: {}, state: {:?}",
                    domain.domain_id, domain.tsm_info.tsm_impl_id, domain.tsm_info.tsm_state
                );
            }
        }
        Err(e) => {
            panic!("Failed to discover domains: {}", e);
        }
    }

    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

/// Get domains
fn discover_and_query_domains() -> Result<heapless::Vec<DomainInfo, MAX_DOMAINS>, &'static str> {
    println!("[SHADOWFAX-HYPERVISOR] enumerating supervisor domains");

    // Get active domains bitmask
    let active_domains = sbi::sbi_call(
        SUPD_EXT_ID,
        SBI_EXT_SUPD_GET_ACTIVE_DOMAINS,
        &[0, 0, 0, 0, 0],
    );

    if active_domains.error < 0 {
        return Err("supervisor domain enumeration failed");
    }

    let domain_mask = active_domains.value;
    let mut domains: heapless::Vec<DomainInfo, MAX_DOMAINS> = heapless::Vec::new();

    println!(
        "[SHADOWFAX-HYPERVISOR] active domain mask: 0x{:016x}",
        domain_mask
    );

    // Single loop to discover domains and query TSM info
    for domain_id in 0..MAX_DOMAINS {
        if (domain_mask & (1 << domain_id)) != 0 {
            println!(
                "[SHADOWFAX-HYPERVISOR] found active domain with ID {}",
                domain_id
            );

            // Create TSM info structure that will be populated by SBI call
            let mut tsm_info = TsmInfo::default();

            // Query TSM info for this domain
            let fid = cove_pack_fid!(domain_id, SBI_EXT_COVH_GET_TSM_INFO as usize);
            let sbi_args = [
                &raw mut tsm_info as *mut TsmInfo as u64,
                size_of::<TsmInfo>() as u64,
                0,
                0,
                0,
            ];

            let tsm_result = sbi_call(COVH_EXT_ID, fid as i32, &sbi_args);

            if tsm_result.error < 0 {
                println!(
                    "[SHADOWFAX-HYPERVISOR] failed to get TSM info for domain {}: error {}",
                    domain_id, tsm_result.error
                );
                continue;
            }

            println!("[SHADOWFAX-HYPERVISOR] domain {} TSM info - impl_id: {}, version: {}, capabilities: 0x{:x}",
                    domain_id, tsm_info.tsm_impl_id, tsm_info.tsm_version, tsm_info.tsm_capabilities);

            // Add to our domain list
            if domains
                .push(DomainInfo {
                    domain_id,
                    tsm_info,
                })
                .is_err()
            {
                println!(
                    "[SHADOWFAX-HYPERVISOR] warning: max domains reached, ignoring domain {}",
                    domain_id
                );
                break;
            }
        }
    }

    println!(
        "[SHADOWFAX-HYPERVISOR] discovered {} active domains",
        domains.len()
    );
    Ok(domains)
}

// Init the hypervisor. We are just launching a bare metal guest.
fn setup_hs_mode(_hartid: usize, _fdt_address: usize) -> ! {
    println!("Starting hypervisor...");
    // clear all hs-mode to vs-mode interrupts.
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

    setup_vs_mode()
}

fn setup_vs_mode() -> ! {
    let mut state = H_STATE.lock();
    state.get_or_init(|| HState::new());

    let guest_entry_point = utils::load_elf(&GUEST_KERNEL);
    let stack_addr = 0x1000;
    unsafe {
        state.get_mut().unwrap().guests.push_unchecked(Guest {
            entry_point: guest_entry_point,
            stack_pointer: stack_addr,
            context_addr: stack_addr - core::mem::size_of::<GuestContext>(),
        });
    }

    hgatp::set(hgatp::Mode::Bare, 0, 0);
    unsafe {
        // sstatus.SUM = 1, sstatus.SPP = 0
        sstatus::set_sum();
        sstatus::set_spp(sstatus::SPP::Supervisor);
        // sstatus.sie = 1
        sstatus::set_sie();
        // sstatus.fs = 1
        sstatus::set_fs(FS::Initial);

        // hstatus.spv = 1 (enable V bit when sret executed)
        hstatus::set_spv();

        // set entry point
        sepc::write(guest_entry_point);

        // // set trap vector
        // assert!(hstrap_vector as *const fn() as usize % 4 == 0);
        // stvec::write(
        //     hstrap_vector as *const fn() as usize,
        //     stvec::TrapMode::Direct,
        // );
        //
        // let mut context = hypervisor_data.get().unwrap().guest().context;
        // context.set_sepc(sepc::read());

        // set sstatus value to context
        // let mut sstatus_val;
        // asm!("csrr {}, sstatus", out(reg) sstatus_val);
        // context.set_sstatus(sstatus_val);
    }
    drop(state);
    guest_entry()
}

#[inline(never)]
fn guest_entry() -> ! {
    let state = H_STATE.lock();
    let guest = state.get().unwrap().guests.first().unwrap();
    let stack_pointer = guest.stack_pointer;
    let context_address = guest.context_addr as *const GuestContext;
    println!(
        "Starting guest: addr: entry_point={:#x}; stack_pointer={:#x}",
        guest.entry_point, stack_pointer
    );

    // release HYPERVISOR_DATA lock
    drop(state);

    unsafe {
        asm!(
            "
            .align 4
            fence.i

            // Restore guest general-purpose registers (GPRs) from context
            mv x0, {x0}
            mv x1, {x1}
            mv x2, {x2}
            mv x3, {x3}
            mv x4, {x4}
            mv x5, {x5}
            mv x6, {x6}
            mv x7, {x7}
            mv x8, {x8}
            mv x9, {x9}
            mv x10, {x10}
            mv x11, {x11}
            mv x12, {x12}
            mv x13, {x13}
            mv x14, {x14}
            mv x15, {x15}
            mv x16, {x16}
            mv x17, {x17}
            mv x18, {x18}
            mv x19, {x19}
            mv x20, {x20}
            mv x21, {x21}
            mv x22, {x22}
            mv x23, {x23}
            mv x24, {x24}
            mv x25, {x25}
            mv x26, {x26}
            mv x27, {x27}
            mv x28, {x28}
            mv x29, {x29}
            mv x30, {x30}
            mv x31, {x31}

            // set sp to scratch stack top
            mv sp, {stack_top}

            sret
            ",
            x0 = in(reg) (*context_address).regs[0],
            x1 = in(reg) (*context_address).regs[1],
            x2 = in(reg) (*context_address).regs[2],
            x3 = in(reg) (*context_address).regs[3],
            x4 = in(reg) (*context_address).regs[4],
            x5 = in(reg) (*context_address).regs[5],
            x6 = in(reg) (*context_address).regs[6],
            x7 = in(reg) (*context_address).regs[7],
            x8 = in(reg) (*context_address).regs[8],
            x9 = in(reg) (*context_address).regs[9],
            x10 = in(reg) (*context_address).regs[10],
            x11 = in(reg) (*context_address).regs[11],
            x12 = in(reg) (*context_address).regs[12],
            x13 = in(reg) (*context_address).regs[13],
            x14 = in(reg) (*context_address).regs[14],
            x15 = in(reg) (*context_address).regs[15],
            x16 = in(reg) (*context_address).regs[16],
            x17 = in(reg) (*context_address).regs[17],
            x18 = in(reg) (*context_address).regs[18],
            x19 = in(reg) (*context_address).regs[19],
            x20 = in(reg) (*context_address).regs[20],
            x21 = in(reg) (*context_address).regs[21],
            x22 = in(reg) (*context_address).regs[22],
            x23 = in(reg) (*context_address).regs[23],
            x24 = in(reg) (*context_address).regs[24],
            x25 = in(reg) (*context_address).regs[25],
            x26 = in(reg) (*context_address).regs[26],
            x27 = in(reg) (*context_address).regs[27],
            x28 = in(reg) (*context_address).regs[28],
            x29 = in(reg) (*context_address).regs[29],
            x30 = in(reg) (*context_address).regs[30],
            x31 = in(reg) (*context_address).regs[31],
            stack_top = in(reg) stack_pointer,
            options(noreturn)
        );
    }
}

mod utils {
    use elf::{abi::PT_LOAD, endian::AnyEndian, segment::ProgramHeader, ElfBytes};
    use heapless::Vec;

    pub fn load_elf(data: &[u8]) -> usize {
        let elf = ElfBytes::<AnyEndian>::minimal_parse(data).unwrap();
        let all_load_phdrs = elf
            .segments()
            .unwrap()
            .iter()
            .filter(|phdr| phdr.p_type == PT_LOAD)
            .collect::<Vec<ProgramHeader, 128>>();

        for segment in all_load_phdrs {
            // Get segment details
            let p_offset = segment.p_offset as usize;
            let p_filesz = segment.p_filesz as usize;
            let p_paddr = segment.p_paddr as *mut u8;
            let p_memsz = segment.p_memsz as usize;
            // Check if the segment data is within bounds
            assert!(
                p_offset + p_filesz <= data.len(),
                "Segment data out of bounds"
            );

            // Copy the segment data to RAM
            let segment_data = &data[p_offset..p_offset + p_filesz];
            unsafe {
                core::ptr::copy_nonoverlapping(segment_data.as_ptr(), p_paddr, p_filesz);
            }
            // zero any .bss past the end of file
            if p_memsz > p_filesz {
                let bss_start = unsafe { p_paddr.add(p_filesz) };
                let bss_len = p_memsz - p_filesz;
                unsafe { core::ptr::write_bytes(bss_start, 0, bss_len) }
            }
        }

        // Return the entry point address of the ELF
        elf.ehdr.e_entry as usize
    }
}

mod sbi {
    #[repr(C)]
    pub struct SbiRet {
        pub error: isize,
        pub value: isize,
    }

    pub fn sbi_call(extid: i32, fid: i32, args: &[u64; 5]) -> SbiRet {
        let (error, value);
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") extid,
                in("a6") fid,
                inlateout("a0") args[0] => error,
                inlateout("a1") args[1] => value,
                in("a2") args[2],
                in("a3") args[3],
                in("a4") args[4],
            );
        }
        SbiRet { error, value }
    }

    pub mod cove {

        pub const COVH_EXT_ID: i32 = 0x434F5648;
        pub const SBI_EXT_COVH_GET_TSM_INFO: i32 = 0;

        pub const SUPD_EXT_ID: i32 = 0x53555044;
        pub const SBI_EXT_SUPD_GET_ACTIVE_DOMAINS: i32 = 0;

        // Assuming these are defined elsewhere
        pub const MAX_DOMAINS: usize = 64;

        #[derive(Clone, Debug)]
        pub enum TsmState {
            /* TSM has not been loaded on this platform. */
            TsmNotLoaded = 0,
            /* TSM has been loaded, but has not yet been initialized. */
            TsmLoaded = 1,
            /* TSM has been loaded & initialized, and is ready to accept ECALLs.*/
            TsmReady = 2,
        }

        #[derive(Debug, Clone)]
        #[repr(C)]
        pub struct TsmInfo {
            pub tsm_state: TsmState,
            pub tsm_impl_id: u32,
            pub tsm_version: u32,
            pub tsm_capabilities: u64,
            pub tvm_state_pages: u64,
            pub tvm_max_vcpus: u32,
            pub tvm_vcpu_state_pages: u64,
        }

        impl Default for TsmInfo {
            fn default() -> Self {
                Self {
                    tsm_state: TsmState::TsmLoaded,
                    tsm_impl_id: 0,
                    tsm_version: 0,
                    tsm_capabilities: 0,
                    tvm_state_pages: 0,
                    tvm_max_vcpus: 0,
                    tvm_vcpu_state_pages: 0,
                }
            }
        }
    }
}
