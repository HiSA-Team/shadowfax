/* This programs prints an helloworld to QEMU Uart. Similarly to C helloworld, it includes
 * the basic init.s. The main has no std neither a main function. Init.s sets up the stack
 * pointer and jumps to the never ending main.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
#![no_std]
#![no_main]

use core::{arch::global_asm, panic::PanicInfo, ptr};

global_asm!(include_str!("init.s"));

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

fn uart_print(message: &str) {
    const UART: *mut u8 = 0x10000000 as *mut u8;

    for c in message.chars() {
        unsafe {
            ptr::write_volatile(UART, c as u8);
        }
    }
}

#[no_mangle]
extern "C" fn main() -> ! {
    uart_print("Shadowfax says: helloworld\n");
    loop {}
}
