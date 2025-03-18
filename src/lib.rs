#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![no_std]
#![no_main]

use core::panic::PanicInfo;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
