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

use core::{
    arch::{asm, global_asm},
    panic::PanicInfo,
    ptr,
};

mod opensbi {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(unused)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

global_asm!(
    r#"
.macro	MOV_3R __d0, __s0, __d1, __s1, __d2, __s2
	add	\__d0, \__s0, zero
	add	\__d1, \__s1, zero
	add	\__d2, \__s2, zero
.endm

.macro	MOV_5R __d0, __s0, __d1, __s1, __d2, __s2, __d3, __s3, __d4, __s4
	add	\__d0, \__s0, zero
	add	\__d1, \__s1, zero
	add	\__d2, \__s2, zero
	add	\__d3, \__s3, zero
	add	\__d4, \__s4, zero
.endm

	.section .payload, "ax", %progbits
	.align 4
	.globl payload_bin
payload_bin:
	wfi
	j	payload_bin
    "#
);

#[link(name = "functions", kind = "static")]
extern "C" {
    // Declare the external function from your static library.
    static _start_warm: u8;
    static _hartid_to_scratch: u8;
}
extern "C" {
    static _payload_start: u8;
    static _fw_start: u8;
    static _fw_end: u8;
    static _fw_rw_start: u8;
    static _bss_start: u8;
    static _bss_end: u8;
}

fn num_to_str<T: Into<u64>>(num: T, buf: &mut [u8]) -> &str {
    let mut i = buf.len();
    let mut num = num.into();
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

#[link_section = ".fw_end"]
static mut STACK: [u8; 1024 * 8] = [0; 1024 * 8];

#[no_mangle]
#[link_section = ".text._start"]
extern "C" fn _start() -> ! {
    unsafe {
        asm!(
            // Load the address of the stack variable into t0.
            "la   t0, {stack}",
            // Jump to the main function// Load the stack size (8K) into t1.
            "li   t1, {stack_size}",
            // Set the stack pointer: sp = t0 + t1.
            "add  sp, t0, t1",
            // Jump to main
            "j    {main}",
            stack = sym STACK,
            // Use the same size as the allocation (8KB)
            stack_size = const opensbi::SBI_SCRATCH_SIZE * 2,
            main = sym main,
            options(noreturn)
        )
    }
}
#[no_mangle]
extern "C" fn main() -> ! {
    //TODO:
    // - inizializzazione opensbi
    // - inizializzazione domain
    // - inizializzazione COVEH

    // reset registers
    // unsafe { asm!("	li	ra, 0", "call	_reset_regs") }

    // zero out bss
    unsafe {
        asm!(
            "la s4, {bss_start}", // Load the address of _bss_start into s4
            "la s5, {bss_end}",   // Load the address of _bss_end into s5
            "0:",
            "sd zero, 0(s4)",     // Store zero to the address in s4
            "addi s4, s4, 8",     // Increment s4 by the size of a double word (8 bytes)
            "blt s4, s5, 0b",     // Loop if s4 is less than s5
            bss_start = sym _bss_start,
            bss_end = sym _bss_end,
        )
    }

    // Platform init correctly configures the "platform" struct
    unsafe {
        asm!(
            "MOV_5R	s0, a0, s1, a1, s2, a2, s3, a3, s4, a4",
            "call	fw_platform_init",
            "add	t0, a0, zero",
            "MOV_5R	a0, s0, a1, s1, a2, s2, a3, s3, a4, s4",
            "add	a1, t0, zero",
        )
    }
    uart_print("opensbi version: ");
    let mut buf = [0u8; 11];
    let major = num_to_str(opensbi::OPENSBI_VERSION_MAJOR, &mut buf);
    uart_print(major);
    uart_print(".");
    let mut buf = [0u8; 11];
    let minor = num_to_str(opensbi::OPENSBI_VERSION_MINOR, &mut buf);
    uart_print(minor);
    uart_print("\n");
    uart_print("hart count: ");
    let mut buf = [0u8; 11];
    let hart_count = num_to_str(unsafe { opensbi::platform.hart_count }, &mut buf);
    uart_print(hart_count);
    uart_print("\n");
    uart_print(unsafe { core::str::from_utf8_unchecked(&*&raw const opensbi::platform.name) });
    uart_print("\n");

    let fw_start_addr = unsafe { &_fw_start as *const u8 as u64 };
    let fw_end_addr = unsafe { &_fw_end as *const u8 as u64 };
    let fw_rw_start_addr = unsafe { &_fw_rw_start as *const u8 as u64 };
    let kernel_addr = unsafe { &_payload_start as *const _ as u64 };

    let start_warm_addr = unsafe { _start_warm as *const u8 as u64 };
    let platform_addr = &raw const opensbi::platform as *const _ as u64;
    let hartid_to_scratch_addr = unsafe { &_hartid_to_scratch as *const u8 as u64 };

    //let sbi_coveh_extension = opensbi::sbi_ecall_extension {
    //    extid_end: SBI_EXT_COVE,
    //    extid_start: SBI_EXT_COVE,
    //    experimental: true,
    //    handle: None,
    //    register_extensions: None,
    //    probe: None,
    //};

    //unsafe {
    //    opensbi::sbi_ecall_register_extension(&mut sbi_coveh_extension);
    //}
    unsafe {
        // calculate heap base address (offset):
        // heap_base = (fw_end + stack_size) - fw_start
        let fw_heap_offset =
            (fw_end_addr + opensbi::platform.hart_stack_size as u64) - fw_start_addr;

        // calculate firmware size :
        // heap_base = (fw_end + stack_size + heap) - fw_start
        let fw_size = (fw_end_addr
            + (opensbi::platform.heap_size + opensbi::platform.hart_stack_size) as u64)
            - fw_start_addr;
        let mut scratch = opensbi::sbi_scratch {
            fw_start: fw_start_addr,
            fw_size: fw_size,
            fw_rw_offset: fw_rw_start_addr - fw_start_addr,
            warmboot_addr: start_warm_addr,
            hartid_to_scratch: hartid_to_scratch_addr,
            fw_heap_offset: fw_heap_offset,
            fw_heap_size: opensbi::platform.heap_size as u64,
            platform_addr: platform_addr,
            next_arg1: 0,
            next_addr: kernel_addr,
            next_mode: 1,
            trap_context: 0,
            tmp0: 0,
            hartindex: 0,
            options: 0,
        };
        opensbi::sbi_init(&mut scratch);
    };
}
