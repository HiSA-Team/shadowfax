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
 * which is used to launch other firmwares.
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
#![no_std]
#![no_main]
use core::{arch::asm, mem, panic::PanicInfo};

mod opensbi {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(unused)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

mod trap;

extern "C" {
    static _fw_start: u8;
    static _fw_end: u8;
    static _fw_rw_start: u8;
    static _bss_start: u8;
    static _bss_end: u8;
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
#[link_section = ".text._start"]
fn _start() -> ! {
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
            stack = sym _fw_end,
            // Use the same size as the allocation (8KB)
            stack_size = const opensbi::SBI_SCRATCH_SIZE * 2,
            main = sym main,
            options(noreturn)
        )
    }
}
#[no_mangle]
fn main() -> ! {
    // zero out bss
    zero_bss();

    // fw_platform_init correctly configures the "platform" struct
    unsafe { asm!("call {fw_platform_init}", fw_platform_init = sym opensbi::fw_platform_init ) }

    init_scratch_space();

    disable_interrupts();

    _start_warm();
    loop {}
}

#[no_mangle]
#[inline]
fn zero_bss() {
    unsafe {
        asm!(
            "la s4, {bss_start}", // Load the address of _bss_start into s4
            "la s5, {bss_end}",   // Load the address of _bss_end into s5
            "0:",
            "sd zero, 0(s4)",     // Store zero to the address in s4
            "addi s4, s4, {pointer_size}",     // Increment s4 by the size of a double word (8 bytes)
            "blt s4, s5, 0b",     // Loop if s4 is less than s5
            bss_start = sym _bss_start,
            bss_end = sym _bss_end,
            pointer_size = const 8,
        )
    }
}

fn reset_regs() {
    unsafe {
        asm!(
            "fence.i",
            "li sp, 0",
            "li gp, 0",
            "li tp, 0",
            "li t0, 0",
            "li t1, 0",
            "li t2, 0",
            "li s0, 0",
            "li s1, 0",
            "li a5, 0",
            "li a6, 0",
            "li a7, 0",
            "li s2, 0",
            "li s3, 0",
            "li s4, 0",
            "li s5, 0",
            "li s6, 0",
            "li s7, 0",
            "li s8, 0",
            "li s9, 0",
            "li s10, 0",
            "li s11, 0",
            "li t3, 0",
            "li t4, 0",
            "li t5, 0",
            "li t6, 0",
            "csrw {csr_mscratch}, 0",
            csr_mscratch = const opensbi::CSR_MSCRATCH,
        )
    }
}

/* Setup scratch space for HART 0
 *
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
 */
#[no_mangle]
fn init_scratch_space() {
    trap::clear_mdt_t0();
    unsafe {
        asm!(
        // get platform details
        "lla a4, {platform}",
        "lwu s7, {sbi_platform_hart_count_offset}(a4)",
        "lwu s8, {sbi_platform_hart_stack_size_offset}(a4)",
        "lwu s9, {sbi_platform_heap_size_offset}(a4)",

        // setup scratch space for all hart
        "lla tp, {fw_end}",
        "mul a5, s7, s8",
        "add tp, tp, a5",

        // setup heap base address
        "lla s10, {fw_start}",
        "sub s10, tp, s10",
        "add tp,  tp, s9",

        // Keep a copy of tp
        "add t3, tp, zero",

        // Hartid 0
        "li t1, 0",

        /*
         * The following registers hold values that are computed before
         * entering this block, and should remain unchanged.
         *
         * t3 -> the firmware end address
         * s8 -> HART stack size
         * s9 -> Heap Size
         * s10 -> Heap Offset
        */
        "add tp, t3, zero",
        "sub tp, tp, s9",
        "mul a5, s8, t1",
        "sub tp, tp, a5",
        "li  a5, {scratch_size}",
        "sub tp, tp, a5",

        // Store fw_start and fw_size in scratch space
        "lla a4, {fw_start}",
        "sub a5, t3, a4",
        "sd a4, {sbi_scratch_fw_start_offset}(tp)",
        "sd a5, {sbi_scratch_fw_size_offset}(tp)",

        // Store R/W sections's offset in scratch space
        "lla a5, {fw_rw_start}",
        "sub a5, a5, a4",
        "sd a5, {sbi_scratch_fw_rw_offset}(tp)",

        // Store fw_heap_offset and fw_heap_size in scratch space
        "sd s10, {sbi_scratch_fw_heap_offset}(tp)",
        "sd s9, {sbi_scratch_fw_heap_size_offset}(tp)",

        // Store next arg1 in scratch space
        "sd a0, {sbi_scratch_next_arg1_offset}(tp)",

        // store next address in scratch space
        "lla a0, {payload}",
        "sd a0, {sbi_scratch_next_addr_offset}(tp)",

        // store next mode in scratch space
        "lla a0, {next_priv}",
        "sd a0, {sbi_scratch_next_mode_offset}(tp)",

        // Store warm_boot address in scratch space
        "lla a0, {warmboot_addr}",
        "sd a0, {sbi_scratch_warmboot_addr_offset}(tp)",

        // store platform address in scratch space
        "lla a4, {platform}",
        "sd a4, {sbi_scratch_platform_addr_offset}(tp)",

        //Store hartid-to-scratch function address in scratch space
        "lla a4, {hartid_to_scratch}",
        "sd a4, {sbi_scratch_hartid_to_scratch_offset}(tp)",

        // Clear trap_context and tmp0 in scratch space
        "sd zero, {sbi_scratch_trap_context_offset}(tp)",
        "sd zero, {sbi_scratch_tmp0_offset}(tp)",

        // Store firmware options in scratch space
        "lla a0, 0",
        "sd a0, {sbi_scratch_options_offset}(tp)",

        // Store hart index in scratch space
        "sd zero, {sbi_scratch_hartindex_offset}(tp)",

        sbi_platform_hart_stack_size_offset = const mem::offset_of!(opensbi::sbi_platform, hart_stack_size),
        sbi_platform_hart_count_offset = const mem::offset_of!(opensbi::sbi_platform, hart_count),
        sbi_platform_heap_size_offset = const mem::offset_of!(opensbi::sbi_platform, heap_size),
        fw_start = sym _fw_start,
        fw_end = sym _fw_end,
        fw_rw_start = sym _fw_rw_start,
        payload = sym kernel,
        next_priv = const 1,
        platform = sym opensbi::platform,
        hartid_to_scratch = sym hartid_to_scratch,
        warmboot_addr = sym _start_warm,
        scratch_size = const opensbi::SBI_SCRATCH_SIZE,
        sbi_scratch_fw_start_offset = const mem::offset_of!(opensbi::sbi_scratch, fw_start),
        sbi_scratch_fw_size_offset = const mem::offset_of!(opensbi::sbi_scratch, fw_size),
        sbi_scratch_fw_rw_offset = const mem::offset_of!(opensbi::sbi_scratch, fw_rw_offset),
        sbi_scratch_fw_heap_offset = const mem::offset_of!(opensbi::sbi_scratch, fw_heap_offset),
        sbi_scratch_fw_heap_size_offset = const mem::offset_of!(opensbi::sbi_scratch, fw_heap_size),
        sbi_scratch_next_arg1_offset = const mem::offset_of!(opensbi::sbi_scratch, next_arg1),
        sbi_scratch_next_addr_offset = const mem::offset_of!(opensbi::sbi_scratch, next_addr),
        sbi_scratch_next_mode_offset = const mem::offset_of!(opensbi::sbi_scratch, next_mode),
        sbi_scratch_warmboot_addr_offset = const mem::offset_of!(opensbi::sbi_scratch, warmboot_addr),
        sbi_scratch_platform_addr_offset = const mem::offset_of!(opensbi::sbi_scratch, platform_addr),
        sbi_scratch_hartid_to_scratch_offset = const mem::offset_of!(opensbi::sbi_scratch, hartid_to_scratch),
        sbi_scratch_trap_context_offset = const mem::offset_of!(opensbi::sbi_scratch, trap_context),
        sbi_scratch_tmp0_offset = const mem::offset_of!(opensbi::sbi_scratch, tmp0),
        sbi_scratch_options_offset = const mem::offset_of!(opensbi::sbi_scratch, options),
        sbi_scratch_hartindex_offset = const mem::offset_of!(opensbi::sbi_scratch, hartindex),
        )
    }
}

#[no_mangle]
#[link_section = ".text._hartid_to_scratch"]
fn hartid_to_scratch() {
    /*
     * a0 -> HART ID (passed by caller)
     * a1 -> HART Index (passed by caller)
     * t0 -> HART Stack Size
     * t1 -> HART Stack End
     * t2 -> Temporary
     */
    unsafe {
        asm!(
            "lla a4, {platform}",
            "lwu t0, {sbi_platform_hart_stack_size_offset}(a4)",
            "lwu t2, {sbi_platform_hart_count_offset}(a4)",
            "sub t2, t2, a1",
            "mul t2, t2, t0",
            "lla t1, {fw_end}",
            "add t1, t1, t2",
            "li t2, {scratch_size}",
            "sub a0, t1, t2",
            platform = sym opensbi::platform,
            sbi_platform_hart_stack_size_offset = const mem::offset_of!(opensbi::sbi_platform, hart_stack_size),
            sbi_platform_hart_count_offset = const mem::offset_of!(opensbi::sbi_platform, hart_count),
            fw_end = sym _fw_end,
            scratch_size = const opensbi::SBI_SCRATCH_SIZE,
        )
    }
}

#[no_mangle]
#[link_section = ".text_start_warm"]
fn _start_warm() {
    unsafe { asm!("li ra, 0") }

    register_extension();
    unsafe {
        asm!(
            // Load platform details: stack offset and hart_index2id
            "lla a4, {platform}",
            "lwu s8, {sbi_platform_hart_stack_size_offset}(a4)",
            "ld s9, {sbi_platform_hart_index2id_offset}(a4)",

            // find the hart space based on index
            "csrr s6, {csr_mhartid}",
            "lla tp, {fw_end}",
            "add tp, tp, s8",
            "li a5, {scratch_size}",
            "sub tp, tp, a5",

            // update mscratch
            "csrw {csr_mscratch}, tp",

            // setup stack
            "add sp, tp, zero",

            // setup trap handler
            "lla a4, {trap_handler}",
            "csrw {csr_mtvec}, a4",

            // clear mdt
            "li t0, 0x0000040000000000",
            "csrc {csr_mstatus}, t0",

            // init sbi for scratch
            "csrr a0, {csr_mscratch}",
            "call {sbi_init}",

            platform = sym opensbi::platform,
            sbi_platform_hart_stack_size_offset = const mem::offset_of!(opensbi::sbi_platform, hart_stack_size),
            sbi_platform_hart_index2id_offset = const mem::offset_of!(opensbi::sbi_platform, hart_index2id),
            csr_mhartid = const opensbi::CSR_MHARTID,
            fw_end = sym _fw_end,
            scratch_size = const opensbi::SBI_SCRATCH_SIZE,
            csr_mscratch = const opensbi::CSR_MSCRATCH,
            trap_handler = sym trap::_trap_handler,
            csr_mtvec = const opensbi::CSR_MTVEC,
            csr_mstatus = const opensbi::CSR_MSTATUS,
            sbi_init = sym opensbi::sbi_init,
        )
    }
}
const COVEH_EXT_NAME: [u8; 8] = *b"coveh  ,";
const COVEH_EXT_ID: u64 = 0x434F5648;

#[link_section = ".text"]
unsafe extern "C" fn sbi_coveh_handler(
    _: u64,
    _: u64,
    _: *mut opensbi::sbi_trap_regs,
    _: *mut opensbi::sbi_ecall_return,
) -> i32 {
    const UART: *mut u8 = 0x10000000 as *mut u8;

    for c in "hello from coveh\n".chars() {
        unsafe {
            core::ptr::write_volatile(UART, c as u8);
        }
    }
    0
}

#[link_section = ".text"]
fn register_extension() {
    let mut extension = opensbi::sbi_ecall_extension {
        experimental: true,
        probe: None,
        name: COVEH_EXT_NAME,
        extid_start: COVEH_EXT_ID,
        extid_end: COVEH_EXT_ID,
        handle: Some(sbi_coveh_handler),
        register_extensions: None,
        head: opensbi::sbi_dlist {
            next: core::ptr::null_mut(),
            prev: core::ptr::null_mut(),
        },
    };

    unsafe { opensbi::sbi_ecall_register_extension(&mut extension) };
}

#[no_mangle]
#[link_section = ".payload.kernel"]
fn kernel() {
    static MSG: [u8; 22] = *b"Hello world shadowfax\n";
    unsafe {
        asm!(
            // First ecall
            "li a7, {extid1}",
            "li a6, {fid1}",
            "li a0, {len}",
            "lla a1, {msg}",
            "li a2, 0",
            "ecall",

            // second ecall
            "li a7, {extid2}",
            "li a2, 0",
            "ecall",

            // params
            extid1 = const 0x4442434E,
            fid1 = const 0x00,
            len = const 22,
            msg = sym MSG,

            extid2 = const COVEH_EXT_ID

        );
    }
    loop {}
}

#[inline(always)]
fn disable_interrupts() {
    unsafe { asm!("csrw {csr_mie}, zero", csr_mie = const opensbi::CSR_MIE ) }
}
