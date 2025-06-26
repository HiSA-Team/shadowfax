#![no_std]
#![no_main]
#![feature(fn_align)]

use core::panic::PanicInfo;

use h_extension::{
    hcounteren,
    hedeleg::{self, ExceptionKind},
    henvcfg, hideleg, hie, hvip, vsatp, VsInterruptKind,
};
use riscv::register::sie;

mod h_extension;
mod log;
mod trap;

#[link_section = ".guest_kernel"]
#[used]
static GUEST_KERNEL: [u8; include_bytes!("../kernel.elf").len()] = *include_bytes!("../kernel.elf");

unsafe extern "C" {
    /// boot stack top (defined in `memory.x`)
    static _top_b_stack: u8;
}

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
            main = sym main,
            options(noreturn)
        )
    }
}

fn main(hartid: usize, fdt_address: usize) -> ! {
    println!("Hypervisor starting...");
    print!("HS-Mode setup...");

    // hart_id must be zero.
    assert_eq!(hartid, 0);

    // dtb_addr test and hint for register usage.
    assert_ne!(fdt_address, 0);
    // clear all hs-mode to vs-mode interrupts.
    hvip::clear(VsInterruptKind::External);
    hvip::clear(VsInterruptKind::Timer);
    hvip::clear(VsInterruptKind::Software);

    // disable address translation.
    vsatp::write(0);

    // enable all hs-mode interrupts
    unsafe {
        sie::set_sext();
        sie::set_ssoft();
        sie::set_stimer();
    }

    // set hie = 0x444
    hie::set(VsInterruptKind::External);
    hie::set(VsInterruptKind::Timer);
    hie::set(VsInterruptKind::Software);

    // enable Sstc extention
    henvcfg::set_stce();
    henvcfg::set_cde();
    henvcfg::set_cbze();
    henvcfg::set_cbcfe();

    // enable hypervisor counter
    hcounteren::set(0xffff_ffff);

    // enable supervisor counter
    unsafe {
        core::arch::asm!("csrw scounteren, {bits}", bits = in(reg) 0xffff_ffff_u32);
    }

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
    print!("completed!\n");

    start_vs_mode(fdt_address)
}

fn start_vs_mode(_fdt_address: usize) -> ! {
    print!("Loading guest kernel...");

    println!("VS-Mode completed!");
    loop {
        riscv::asm::wfi();
    }
}
