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
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
#![no_std]
#![no_main]
const SBI_PLATFORM_HART_STACK_SIZE_OFFSET: usize =
    mem::offset_of!(opensbi::sbi_platform, hart_stack_size);
const SBI_PLATFORM_HART_INDEX2ID_OFFSET: usize =
    mem::offset_of!(opensbi::sbi_platform, hart_index2id);
const SBI_PLATFORM_HART_COUNT_OFFSET: usize = mem::offset_of!(opensbi::sbi_platform, hart_count);
const SBI_PLATFORM_HEAP_SIZE_OFFSET: usize = mem::offset_of!(opensbi::sbi_platform, heap_size);
const SBI_SCRATCH_FW_START_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, fw_start);
const SBI_SCRATCH_FW_SIZE_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, fw_size);
const SBI_SCRATCH_FW_RW_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, fw_rw_offset);
const SBI_SCRATCH_FW_HEAP_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, fw_heap_offset);
const SBI_SCRATCH_FW_HEAP_SIZE_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, fw_heap_size);
const SBI_SCRATCH_NEXT_ARG1_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, next_arg1);
const SBI_SCRATCH_NEXT_ADDR_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, next_addr);
const SBI_SCRATCH_NEXT_MODE_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, next_mode);
const SBI_SCRATCH_WARMBOOT_ADDR_OFFSET: usize =
    mem::offset_of!(opensbi::sbi_scratch, warmboot_addr);
const SBI_SCRATCH_PLATFORM_ADDR_OFFSET: usize =
    mem::offset_of!(opensbi::sbi_scratch, platform_addr);
const SBI_SCRATCH_HARTID_TO_SCRATCH_OFFSET: usize =
    mem::offset_of!(opensbi::sbi_scratch, hartid_to_scratch);

const SBI_SCRATCH_TRAP_CONTEXT_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, trap_context);
const SBI_SCRATCH_TMP0_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, tmp0);
const SBI_SCRATCH_OPTIONS_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, options);
const SBI_SCRATCH_HARTINDEX_OFFSET: usize = mem::offset_of!(opensbi::sbi_scratch, hartindex);

use core::{arch::asm, mem, panic::PanicInfo, ptr};

mod opensbi {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(unused)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
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

#[no_mangle]
#[inline(always)]
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
            stack = sym _fw_end,
            // Use the same size as the allocation (8KB)
            stack_size = const opensbi::SBI_SCRATCH_SIZE * 2,
            main = sym main,
            options(noreturn)
        )
    }
}
#[no_mangle]
extern "C" fn main() -> ! {
    // zero out bss
    zero_bss();

    // fw_platform_init correctly configures the "platform" struct
    unsafe { asm!("call fw_platform_init") }

    // dump_config();

    init_scratch_space();

    disable_interrupts();

    _start_warm();
    loop {}
}

#[no_mangle]
#[inline]
extern "C" fn zero_bss() {
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

#[no_mangle]
extern "C" fn reset_regs() {
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
extern "C" fn init_scratch_space() {
    clear_mdt_t0();
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

        sbi_platform_hart_stack_size_offset = const SBI_PLATFORM_HART_STACK_SIZE_OFFSET,
        sbi_platform_hart_count_offset = const SBI_PLATFORM_HART_COUNT_OFFSET,
        sbi_platform_heap_size_offset = const SBI_PLATFORM_HEAP_SIZE_OFFSET,
        fw_start = sym _fw_start,
        fw_end = sym _fw_end,
        fw_rw_start = sym _fw_rw_start,
        payload = sym payload,
        next_priv = const 1,
        platform = sym opensbi::platform,
        hartid_to_scratch = sym hartid_to_scratch,
        warmboot_addr = sym _start_warm,
        scratch_size = const opensbi::SBI_SCRATCH_SIZE,
        sbi_scratch_fw_start_offset = const SBI_SCRATCH_FW_START_OFFSET,
        sbi_scratch_fw_size_offset = const SBI_SCRATCH_FW_SIZE_OFFSET,
        sbi_scratch_fw_rw_offset = const SBI_SCRATCH_FW_RW_OFFSET,
        sbi_scratch_fw_heap_offset = const SBI_SCRATCH_FW_HEAP_OFFSET,
        sbi_scratch_fw_heap_size_offset = const SBI_SCRATCH_FW_HEAP_SIZE_OFFSET,
        sbi_scratch_next_arg1_offset = const SBI_SCRATCH_NEXT_ARG1_OFFSET,
        sbi_scratch_next_addr_offset = const SBI_SCRATCH_NEXT_ADDR_OFFSET,
        sbi_scratch_next_mode_offset = const SBI_SCRATCH_NEXT_MODE_OFFSET,
        sbi_scratch_warmboot_addr_offset = const SBI_SCRATCH_WARMBOOT_ADDR_OFFSET,
        sbi_scratch_platform_addr_offset = const SBI_SCRATCH_PLATFORM_ADDR_OFFSET,
        sbi_scratch_hartid_to_scratch_offset = const SBI_SCRATCH_HARTID_TO_SCRATCH_OFFSET,
        sbi_scratch_trap_context_offset = const SBI_SCRATCH_TRAP_CONTEXT_OFFSET,
        sbi_scratch_tmp0_offset = const SBI_SCRATCH_TMP0_OFFSET,
        sbi_scratch_options_offset = const SBI_SCRATCH_OPTIONS_OFFSET,
        sbi_scratch_hartindex_offset = const SBI_SCRATCH_HARTINDEX_OFFSET,
        )
    }
}

#[no_mangle]
extern "C" fn start_hang() {
    loop {}
}

#[no_mangle]
extern "C" fn dump_config() {
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
}

#[no_mangle]
#[link_section = ".text._hartid_to_scratch"]
extern "C" fn hartid_to_scratch() {
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
            sbi_platform_hart_stack_size_offset = const SBI_PLATFORM_HART_STACK_SIZE_OFFSET,
            sbi_platform_hart_count_offset = const SBI_PLATFORM_HART_COUNT_OFFSET,
            fw_end = sym _fw_end,
            scratch_size = const opensbi::SBI_SCRATCH_SIZE,
        )
    }
}

#[no_mangle]
#[link_section = ".text_start_warm"]
extern "C" fn _start_warm() {
    unsafe { asm!("li ra, 0") }
    // reset_regs();

    disable_interrupts();

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

            // init sbi for scratch
            "csrr a0, {csr_mscratch}",
            "call {sbi_init}",

            platform = sym opensbi::platform,
            sbi_platform_hart_stack_size_offset = const SBI_PLATFORM_HART_STACK_SIZE_OFFSET,
            sbi_platform_hart_index2id_offset = const SBI_PLATFORM_HART_INDEX2ID_OFFSET,
            csr_mhartid = const opensbi::CSR_MHARTID,
            fw_end = sym _fw_end,
            scratch_size = const opensbi::SBI_SCRATCH_SIZE,
            csr_mscratch = const opensbi::CSR_MSCRATCH,
            trap_handler = sym _trap_handler,
            csr_mtvec = const opensbi::CSR_MTVEC,
            sbi_init = sym opensbi::sbi_init,

        )
    }
}

#[no_mangle]
#[link_section = ".payload"]
extern "C" fn payload() {
    unsafe { asm!("li a7, 0x53555044", "li a6, 0x0", "ecall") }
    loop {}
}

#[inline(always)]
#[no_mangle]
extern "C" fn disable_interrupts() {
    unsafe { asm!("csrw {csr_mie}, zero", csr_mie = const opensbi::CSR_MIE ) }
}

#[inline(always)]
#[no_mangle]
#[link_section = ".entry"]
extern "C" fn _trap_handler() {
    trap_save_and_setup_sp_t0();
    trap_save_mepc_status();
    trap_save_general_regs_except_sp_t0();
    trap_save_info();
    trap_call_c_routine();
    trap_restore_general_regs_except_a0_t0();
    trap_restore_mepc_status();
    trap_restore_a0_t0()
}

#[no_mangle]
#[inline(always)]
extern "C" fn trap_save_and_setup_sp_t0() {
    unsafe {
        asm!(
            //  Swap TP and MSCRATCH
            "csrrw tp, {csr_mscratch}, tp",
            "sd t0, {sbi_scratch_tmp0_offset}(tp)",
            /*
             * Set T0 to appropriate exception stack
             *
             * Came_From_M_Mode = ((MSTATUS.MPP < PRV_M) ? 1 : 0) - 1;
             * Exception_Stack = TP ^ (Came_From_M_Mode & (SP ^ TP))
             *
             * Came_From_M_Mode = 0    ==>    Exception_Stack = TP
             * Came_From_M_Mode = -1   ==>    Exception_Stack = SP
             */
            "csrr t0, {csr_mstatus}",
            "srl t0, t0, {mstatus_mpp_shift}",
            "and t0, t0, {priv_m}",
            "slti t0, t0, {priv_m}",
            "add t0, t0, -1",
            "xor sp, sp, tp",
            "and t0, t0, sp",
            "xor sp, sp, tp",
            "xor t0, tp, t0",
            // Save original SP on exception st
            "sd sp,  ({sbi_trap_regs_offset_tp}-{sbi_trap_context_size})(t0)",
            // Set SP to exception stack and make room for trap context
            "add sp, t0, -{sbi_trap_context_size}",
            // Restore T0 from scratch space
            "ld t0, {sbi_scratch_tmp0_offset}(tp)",
            // Save T0 on stack
            "sd t0, {sbi_trap_regs_offset_t0}(sp)",
            // Swap TP and MSCRATCH
            "csrrw tp, {csr_mscratch}, tp",

            csr_mscratch = const opensbi::CSR_MSCRATCH,
            csr_mstatus = const opensbi::CSR_MSTATUS,
            mstatus_mpp_shift = const opensbi::MSTATUS_MPP_SHIFT,
            sbi_trap_context_size = const (size_of::<opensbi::sbi_trap_regs>() +size_of::<opensbi::sbi_trap_info>() + 8),
            sbi_trap_regs_offset_tp = const opensbi::SBI_TRAP_REGS_tp * 8,
            sbi_trap_regs_offset_t0 = const opensbi::SBI_TRAP_REGS_t0 * 8,
            sbi_scratch_tmp0_offset = const SBI_SCRATCH_TMP0_OFFSET,
            priv_m = const 3,

        )
    }
}
#[no_mangle]
#[inline(always)]
extern "C" fn trap_save_mepc_status() {
    unsafe {
        asm!(
            "csrr t0, {csr_mepc}",
            "sd t0, {sbi_trap_regs_offset_mepc}(sp)",
            "csrr t0, {csr_mstatus}",
            "sd t0, {sbi_trap_regs_offset_mstatus}(sp)",
            "sd zero, {sbi_trap_regs_offset_mstatush}(sp)",
            csr_mstatus = const opensbi::CSR_MSTATUS,
            csr_mepc= const opensbi::CSR_MEPC,
            sbi_trap_regs_offset_mepc = const opensbi::SBI_TRAP_REGS_mepc * 8,
            sbi_trap_regs_offset_mstatus= const opensbi::SBI_TRAP_REGS_mstatus * 8,
            sbi_trap_regs_offset_mstatush = const opensbi::SBI_TRAP_REGS_mstatusH * 8,
        )
    }
}

#[no_mangle]
#[inline(always)]
extern "C" fn trap_save_general_regs_except_sp_t0() {
    unsafe {
        asm!(
            "sd zero, {sbi_trap_regs_offset_zero}(sp)",
            "sd ra, {sbi_trap_regs_offset_ra}(sp)",
            "sd gp, {sbi_trap_regs_offset_gp}(sp)",
            "sd tp, {sbi_trap_regs_offset_tp}(sp)",
            "sd t1, {sbi_trap_regs_offset_t1}(sp)",
            "sd t2, {sbi_trap_regs_offset_t2}(sp)",
            "sd s0, {sbi_trap_regs_offset_s0}(sp)",
            "sd s1, {sbi_trap_regs_offset_s1}(sp)",
            "sd a0, {sbi_trap_regs_offset_a0}(sp)",
            "sd a1, {sbi_trap_regs_offset_a1}(sp)",
            "sd a2, {sbi_trap_regs_offset_a2}(sp)",
            "sd a3, {sbi_trap_regs_offset_a3}(sp)",
            "sd a4, {sbi_trap_regs_offset_a4}(sp)",
            "sd a5, {sbi_trap_regs_offset_a5}(sp)",
            "sd a6, {sbi_trap_regs_offset_a6}(sp)",
            "sd a7, {sbi_trap_regs_offset_a7}(sp)",
            "sd s2, {sbi_trap_regs_offset_s2}(sp)",
            "sd s3, {sbi_trap_regs_offset_s3}(sp)",
            "sd s4, {sbi_trap_regs_offset_s4}(sp)",
            "sd s5, {sbi_trap_regs_offset_s5}(sp)",
            "sd s6, {sbi_trap_regs_offset_s6}(sp)",
            "sd s7, {sbi_trap_regs_offset_s7}(sp)",
            "sd s8, {sbi_trap_regs_offset_s8}(sp)",
            "sd s9, {sbi_trap_regs_offset_s9}(sp)",
            "sd s10, {sbi_trap_regs_offset_s10}(sp)",
            "sd s11, {sbi_trap_regs_offset_s11}(sp)",
            "sd t3, {sbi_trap_regs_offset_t3}(sp)",
            "sd t4, {sbi_trap_regs_offset_t4}(sp)",
            "sd t5, {sbi_trap_regs_offset_t5}(sp)",
            "sd t6, {sbi_trap_regs_offset_t6}(sp)",
            sbi_trap_regs_offset_zero = const opensbi::SBI_TRAP_REGS_zero * 8,
            sbi_trap_regs_offset_ra = const opensbi::SBI_TRAP_REGS_ra * 8,
            sbi_trap_regs_offset_gp = const opensbi::SBI_TRAP_REGS_gp * 8,
            sbi_trap_regs_offset_tp = const opensbi::SBI_TRAP_REGS_tp * 8,
            sbi_trap_regs_offset_t1 = const opensbi::SBI_TRAP_REGS_t1 * 8,
            sbi_trap_regs_offset_t2 = const opensbi::SBI_TRAP_REGS_t2 * 8,
            sbi_trap_regs_offset_s0 = const opensbi::SBI_TRAP_REGS_s0 * 8,
            sbi_trap_regs_offset_s1 = const opensbi::SBI_TRAP_REGS_s1 * 8,
            sbi_trap_regs_offset_a0 = const opensbi::SBI_TRAP_REGS_a0 * 8,
            sbi_trap_regs_offset_a1 = const opensbi::SBI_TRAP_REGS_a1 * 8,
            sbi_trap_regs_offset_a2 = const opensbi::SBI_TRAP_REGS_a2 * 8,
            sbi_trap_regs_offset_a3 = const opensbi::SBI_TRAP_REGS_a3 * 8,
            sbi_trap_regs_offset_a4 = const opensbi::SBI_TRAP_REGS_a4 * 8,
            sbi_trap_regs_offset_a5 = const opensbi::SBI_TRAP_REGS_a5 * 8,
            sbi_trap_regs_offset_a6 = const opensbi::SBI_TRAP_REGS_a6 * 8,
            sbi_trap_regs_offset_a7 = const opensbi::SBI_TRAP_REGS_a7 * 8,
            sbi_trap_regs_offset_s2 = const opensbi::SBI_TRAP_REGS_s2 * 8,
            sbi_trap_regs_offset_s3 = const opensbi::SBI_TRAP_REGS_s3 * 8,
            sbi_trap_regs_offset_s4 = const opensbi::SBI_TRAP_REGS_s4 * 8,
            sbi_trap_regs_offset_s5 = const opensbi::SBI_TRAP_REGS_s5 * 8,
            sbi_trap_regs_offset_s6 = const opensbi::SBI_TRAP_REGS_s6 * 8,
            sbi_trap_regs_offset_s7 = const opensbi::SBI_TRAP_REGS_s7 * 8,
            sbi_trap_regs_offset_s8 = const opensbi::SBI_TRAP_REGS_s8 * 8,
            sbi_trap_regs_offset_s9 = const opensbi::SBI_TRAP_REGS_s9 * 8,
            sbi_trap_regs_offset_s10 = const opensbi::SBI_TRAP_REGS_s10 * 8,
            sbi_trap_regs_offset_s11 = const opensbi::SBI_TRAP_REGS_s11 * 8,
            sbi_trap_regs_offset_t3 = const opensbi::SBI_TRAP_REGS_t3 * 8,
            sbi_trap_regs_offset_t4 = const opensbi::SBI_TRAP_REGS_t4 * 8,
            sbi_trap_regs_offset_t5 = const opensbi::SBI_TRAP_REGS_t5 * 8,
            sbi_trap_regs_offset_t6 = const opensbi::SBI_TRAP_REGS_t6 * 8,
        )
    }
}
#[no_mangle]
#[inline(always)]
extern "C" fn trap_save_info() {
    unsafe {
        asm!(
            "csrr t0, {csr_mcause}",
            "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_cause})(sp)",
            "csrr t0, {csr_mtval}",
            "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tval})(sp)",
            "sd zero, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tval2})(sp)",
            "sd zero, ({sbi_trap_regs_size} + {sbi_trap_info_offset_tinst})(sp)",
            "li t0, 0",
            "sd t0, ({sbi_trap_regs_size} + {sbi_trap_info_offset_gva})(sp)",
            csr_mcause = const opensbi::CSR_MCAUSE,
            csr_mtval = const opensbi::CSR_MTVAL,
            sbi_trap_regs_size = const 8 * opensbi::SBI_TRAP_REGS_last,
            sbi_trap_info_offset_cause = const 8 * opensbi::SBI_TRAP_INFO_cause,
            sbi_trap_info_offset_tval = const 8 * opensbi::SBI_TRAP_INFO_tval,
            sbi_trap_info_offset_tval2 = const 8 * opensbi::SBI_TRAP_INFO_tval2,
            sbi_trap_info_offset_tinst = const 8 * opensbi::SBI_TRAP_INFO_tinst,
            sbi_trap_info_offset_gva = const 8 * opensbi::SBI_TRAP_INFO_gva,
        )
    };
    clear_mdt_t0();
}
#[no_mangle]
#[inline(always)]
extern "C" fn trap_call_c_routine() {
    unsafe {
        asm!("add a0, sp, zero", "call {sbi_trap_handler}", sbi_trap_handler = sym opensbi::sbi_trap_handler)
    }
}
#[no_mangle]
#[inline(always)]
extern "C" fn trap_restore_general_regs_except_a0_t0() {
    unsafe {
        asm!(
            "ld ra, {sbi_trap_regs_offset_ra}(a0)",
            "ld sp, {sbi_trap_regs_offset_sp}(a0)",
            "ld gp, {sbi_trap_regs_offset_gp}(a0)",
            "ld tp, {sbi_trap_regs_offset_tp}(a0)",
            "ld t1, {sbi_trap_regs_offset_t1}(a0)",
            "ld t2, {sbi_trap_regs_offset_t2}(a0)",
            "ld s0, {sbi_trap_regs_offset_s0}(a0)",
            "ld s1, {sbi_trap_regs_offset_s1}(a0)",
            "ld a1, {sbi_trap_regs_offset_a1}(a0)",
            "ld a2, {sbi_trap_regs_offset_a2}(a0)",
            "ld a3, {sbi_trap_regs_offset_a3}(a0)",
            "ld a4, {sbi_trap_regs_offset_a4}(a0)",
            "ld a5, {sbi_trap_regs_offset_a5}(a0)",
            "ld a6, {sbi_trap_regs_offset_a6}(a0)",
            "ld a7, {sbi_trap_regs_offset_a7}(a0)",
            "ld s2, {sbi_trap_regs_offset_s2}(a0)",
            "ld s3, {sbi_trap_regs_offset_s3}(a0)",
            "ld s4, {sbi_trap_regs_offset_s4}(a0)",
            "ld s5, {sbi_trap_regs_offset_s5}(a0)",
            "ld s6, {sbi_trap_regs_offset_s6}(a0)",
            "ld s7, {sbi_trap_regs_offset_s7}(a0)",
            "ld s8, {sbi_trap_regs_offset_s8}(a0)",
            "ld s9, {sbi_trap_regs_offset_s9}(a0)",
            "ld s10, {sbi_trap_regs_offset_s10}(a0)",
            "ld s11, {sbi_trap_regs_offset_s11}(a0)",
            "ld t3, {sbi_trap_regs_offset_t3}(a0)",
            "ld t4, {sbi_trap_regs_offset_t4}(a0)",
            "ld t5, {sbi_trap_regs_offset_t5}(a0)",
            "ld t6, {sbi_trap_regs_offset_t6}(a0)",
            sbi_trap_regs_offset_sp = const opensbi::SBI_TRAP_REGS_sp * 8,
            sbi_trap_regs_offset_ra = const opensbi::SBI_TRAP_REGS_ra * 8,
            sbi_trap_regs_offset_gp = const opensbi::SBI_TRAP_REGS_gp * 8,
            sbi_trap_regs_offset_tp = const opensbi::SBI_TRAP_REGS_tp * 8,
            sbi_trap_regs_offset_t1 = const opensbi::SBI_TRAP_REGS_t1 * 8,
            sbi_trap_regs_offset_t2 = const opensbi::SBI_TRAP_REGS_t2 * 8,
            sbi_trap_regs_offset_s0 = const opensbi::SBI_TRAP_REGS_s0 * 8,
            sbi_trap_regs_offset_s1 = const opensbi::SBI_TRAP_REGS_s1 * 8,
            sbi_trap_regs_offset_a1 = const opensbi::SBI_TRAP_REGS_a1 * 8,
            sbi_trap_regs_offset_a2 = const opensbi::SBI_TRAP_REGS_a2 * 8,
            sbi_trap_regs_offset_a3 = const opensbi::SBI_TRAP_REGS_a3 * 8,
            sbi_trap_regs_offset_a4 = const opensbi::SBI_TRAP_REGS_a4 * 8,
            sbi_trap_regs_offset_a5 = const opensbi::SBI_TRAP_REGS_a5 * 8,
            sbi_trap_regs_offset_a6 = const opensbi::SBI_TRAP_REGS_a6 * 8,
            sbi_trap_regs_offset_a7 = const opensbi::SBI_TRAP_REGS_a7 * 8,
            sbi_trap_regs_offset_s2 = const opensbi::SBI_TRAP_REGS_s2 * 8,
            sbi_trap_regs_offset_s3 = const opensbi::SBI_TRAP_REGS_s3 * 8,
            sbi_trap_regs_offset_s4 = const opensbi::SBI_TRAP_REGS_s4 * 8,
            sbi_trap_regs_offset_s5 = const opensbi::SBI_TRAP_REGS_s5 * 8,
            sbi_trap_regs_offset_s6 = const opensbi::SBI_TRAP_REGS_s6 * 8,
            sbi_trap_regs_offset_s7 = const opensbi::SBI_TRAP_REGS_s7 * 8,
            sbi_trap_regs_offset_s8 = const opensbi::SBI_TRAP_REGS_s8 * 8,
            sbi_trap_regs_offset_s9 = const opensbi::SBI_TRAP_REGS_s9 * 8,
            sbi_trap_regs_offset_s10 = const opensbi::SBI_TRAP_REGS_s10 * 8,
            sbi_trap_regs_offset_s11 = const opensbi::SBI_TRAP_REGS_s11 * 8,
            sbi_trap_regs_offset_t3 = const opensbi::SBI_TRAP_REGS_t3 * 8,
            sbi_trap_regs_offset_t4 = const opensbi::SBI_TRAP_REGS_t4 * 8,
            sbi_trap_regs_offset_t5 = const opensbi::SBI_TRAP_REGS_t5 * 8,
            sbi_trap_regs_offset_t6 = const opensbi::SBI_TRAP_REGS_t6 * 8,
        )
    }
}

#[no_mangle]
#[inline(always)]
extern "C" fn trap_restore_mepc_status() {
    unsafe {
        asm!(
            "ld t0, {sbi_trap_regs_offset_mstatus}(a0)",
            "csrw {csr_mstatus}, t0",
            "ld t0, {sbi_trap_regs_offset_mepc}(a0)",
            "csrw {csr_mepc}, t0",
            csr_mstatus = const opensbi::CSR_MSTATUS,
            csr_mepc= const opensbi::CSR_MEPC,
            sbi_trap_regs_offset_mepc = const opensbi::SBI_TRAP_REGS_mepc * 8,
            sbi_trap_regs_offset_mstatus= const opensbi::SBI_TRAP_REGS_mstatus * 8,
        )
    }
}
#[no_mangle]
#[inline(always)]
extern "C" fn trap_restore_a0_t0() {
    unsafe {
        asm!(
            "ld t0, {sbi_trap_regs_offset_t0}(a0)",
            "ld a0, {sbi_trap_regs_offset_a0}(a0)",
            sbi_trap_regs_offset_t0 = const opensbi::SBI_TRAP_REGS_t0 * 8,
            sbi_trap_regs_offset_a0 = const opensbi::SBI_TRAP_REGS_a0 * 8,
        )
    }
}
#[no_mangle]
#[inline(always)]
extern "C" fn clear_mdt_t0() {
    unsafe {
        asm!(
            "li t0, 0x40000000000",
            "csrc {csr_mstatus}, t0",
            csr_mstatus = const opensbi::CSR_MSTATUS,
        )
    }
}
