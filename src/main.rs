/* Shadowfax entry point. This codes initializes the platform and jumps to main function using **opensbi** as a
 * static library (https://github.com/riscv-software-src/opensbi/blob/master/docs/library_usage.md).
 * According to opensbi static library, there are two constraints required before calling init functions:
 * - The RISC-V MSCRATCH CSR must point to a valid OpenSBI scratch space (i.e. a struct sbi_scratch instance);
 * - The RISC-V SP register (i.e. the stack pointer) must be set per-HART pointing to distinct non-overlapping stacks.
 *
 * The external firmware or bootloader must ensure that interrupts are disabled in the MSTATUS and MIE CSRs
 * when calling the functions sbi_init() and sbi_trap_handler().
 *
 * Part of this code is taken from https://github.com/riscv-software-src/opensbi/blob/master/firmware/fw_base.S
 * which is used to launch other firmware types with S or HS mode.
 *
 * A scratch is a per hart data structure reported below defined in <sbi/sbi_scratch.h>
 *
 * struct sbi_scratch {
 *   unsigned long fw_start;
 *   unsigned long fw_size;
 *   unsigned long fw_rw_offset;
 *   unsigned long fw_heap_offset;
 *   unsigned long fw_heap_size;
 *   unsigned long next_arg1;
 *   unsigned long next_addr;
 *   unsigned long next_mode;
 *   unsigned long warmboot_addr;
 *   unsigned long platform_addr;
 *   unsigned long hartid_to_scratch;
 *   unsigned long trap_context;
 *   unsigned long tmp0;
 *   unsigned long options;
 *   unsigned long hartindex;
 * };
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
#![no_std]
#![no_main]

use core::{arch::global_asm, panic::PanicInfo, ptr};

global_asm!(include_str!("../examples/common/init.S"));

mod opensbi {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(unused)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
fn u32_to_str(mut num: u32, buf: &mut [u8; 11]) -> &str {
    // u32 max is 10 digits so we allocate 11 bytes for safety.
    let mut i = buf.len();
    if num == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while num > 0 {
            i -= 1;
            buf[i] = b'0' + (num % 10) as u8;
            num /= 10;
        }
    }
    // Safety: The buffer only contains valid ASCII digits.
    unsafe { core::str::from_utf8_unchecked(&buf[i..]) }
}

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

const SBI_EXT_COVE: u64 = 0x53555044;

#[no_mangle]
extern "C" fn main() -> ! {
    //TODO:
    // - inizializzazione opensbi
    // - inizializzazione domain
    // - inizializzazione COVEH
    let mut buf = [0u8; 11];
    let major = u32_to_str(opensbi::OPENSBI_VERSION_MAJOR, &mut buf);
    let mut buf = [0u8; 11];
    let minor = u32_to_str(opensbi::OPENSBI_VERSION_MINOR, &mut buf);
    uart_print("opensbi version: ");
    uart_print(major);
    uart_print(".");
    uart_print(minor);
    uart_print("\n");

    //let sbi_coveh_extension = opensbi::sbi_ecall_extension {
    //    extid_end: SBI_EXT_COVE,
    //    extid_start: SBI_EXT_COVE,
    //    experimental: true,
    //    handle: None,
    //    register_extensions: None,
    //    probe: None,
    //};
    //
    //unsafe {
    //    opensbi::sbi_ecall_register_extension(&mut sbi_coveh_extension);
    //}

    loop {}
}
