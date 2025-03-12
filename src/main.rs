/*  This will be shadowfax entrypoint.
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
#![no_std]
#![no_main]

use core::{arch::global_asm, panic::PanicInfo, ptr};

global_asm!(include_str!("init.S"));

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}


#[no_mangle]
extern "C" fn main() -> ! {
    loop {}
}
