#![no_std]
#![no_main]
#![feature(fn_align)]

use core::panic::PanicInfo;

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

// Init the hypervisor. We are just launching a bare metal guest.
fn main() -> ! {
    loop {}
}
