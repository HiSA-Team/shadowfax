#![no_std]
#![no_main]
#![feature(fn_align)]

use core::{arch::asm, cell::OnceCell, panic::PanicInfo};

use elf::{abi::PT_LOAD, endian::AnyEndian, segment::ProgramHeader, ElfBytes};
use h_extension::{
    csrs::{
        hcounteren,
        hedeleg::{self, ExceptionKind},
        henvcfg, hgatp, hideleg, hie, hstatus, hvip, vsatp, VsInterruptKind,
    },
    instruction::hfence_gvma_all,
};
use heapless::Vec;
use riscv::register::{
    sepc, sscratch,
    sstatus::{self, FS},
};
use spin::Mutex;

mod h_extension;
mod log;

#[link_section = ".guest_kernel"]
#[used]
static GUEST_KERNEL: [u8; include_bytes!("../kernel.elf").len()] = *include_bytes!("../kernel.elf");

unsafe extern "C" {
    /// boot stack top (defined in `memory.x`)
    static _top_b_stack: u8;
    static _stack_start: u8;
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
    guests: Vec<Guest, MAX_NUM_GUESTS>,
}

impl HState {
    fn new() -> Self {
        Self { guests: Vec::new() }
    }
}

static H_STATE: Mutex<OnceCell<HState>> = Mutex::new(OnceCell::new());

/*
 * This is needed for rust bare metal programs
 */
#[inline(never)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// Give each hart 8K stack
const STACK_SIZE_PER_HART: usize = 1024 * 8;

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
            main = sym setup_hs_mode,
            options(noreturn)
        )
    }
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
    let mut state = unsafe { H_STATE.lock() };
    state.get_or_init(|| HState::new());

    let guest_entry_point = load_elf(&GUEST_KERNEL);
    let stack_addr = unsafe { &_stack_start as *const u8 as usize };
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
    println!(
        "Starting guest: addr: entry_point={:#x}; stack_pointer={:#x}",
        guest.entry_point, stack_pointer
    );

    // release HYPERVISOR_DATA lock
    drop(state);

    unsafe {
        // enter VS-mode
        asm!(
            ".align 4
            fence.i

            // set sp to scratch stack top
            mv sp, {stack_top}

            sret
            ",
            stack_top = in(reg) stack_pointer,
            options(noreturn)
        );
    }
}

fn load_elf(data: &[u8]) -> usize {
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
