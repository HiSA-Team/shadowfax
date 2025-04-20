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
 *
 * This firmware starts from _start, sets up the stack pointer and jumps to a never returning main
 * function.
 *
 * Shadowfax will register opensbi CoVE extensions. There are 4 extensions:
 *  - covh: extension to be used from host;
 *  - covi: extension to manage interrupts;
 *  - covg: extension used from the guest to access firmware level services;
 *  - supd: supervisor domain extension;
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
#![no_std]
#![no_main]
use core::{arch::asm, mem, panic::PanicInfo};

mod cove;

/* This module includes the `bindings.rs` generated
 * using `build.rs` which translates opensbi C definitions
 * in Rust. This could be also be included without the module,
 * but doing in this way mandates that every opensbi symbol
 * is used with `opensbi::<symbol>`.
 */
mod opensbi {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(unused)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

mod trap;

// This "object" is just to hold symbols declared in the linkerscript
// In `linker.ld`, we define this values and this is a way to access them
// from Rust.
unsafe extern "C" {
    static _fw_start: u8;
    static _fw_end: u8;
    static _fw_rw_start: u8;
    static _bss_start: u8;
    static _bss_end: u8;
}

/// This is needed for rust bare metal programs
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// The _start function is the first function
/// loaded at the starting address of the linkerscript
/// Here we setup a temporary stack at the end of the firmware
/// and jump to main
#[no_mangle]
#[link_section = ".text._start"]
fn _start() -> ! {
    unsafe {
        asm!(
            // If there are multiple hart, init only hartid 0
            "csrr s6, {csr_mhartid}",
            // If not zero, go to wait loop
            "bnez s6, {wait_for_boot_hart}",
            // Load the address of the stack variable into t0.
            "la   t0, {stack}",
            // Jump to the main function// Load the stack size (8K) into t1.
            "li   t1, {stack_size}",
            // Set the stack pointer: sp = t0 + t1.
            "add  sp, t0, t1",
            // Jump to main
            "j    {main}",
            csr_mhartid = const opensbi::CSR_MHARTID,
            wait_for_boot_hart = sym start_hang,
            stack = sym _fw_end,
            // Use the same size as the allocation (8KB)
            stack_size = const opensbi::SBI_SCRATCH_SIZE * 2,
            main = sym main,
            options(noreturn)
        )
    }
}

/// The main function serves as the entry point for the firmware execution. It performs
/// several critical initialization tasks to prepare the system for operation. These tasks
/// include zeroing out the BSS section, setting up a temporary trap handler, initializing
/// the platform, configuring the scratch space, and starting the warm boot process.
///
/// # Safety
///
/// This function is marked as unsafe because it involves direct manipulation of machine-level
/// registers and relies on specific memory layout assumptions. It should only be called in a
/// controlled environment where these assumptions hold true.
fn main() -> ! {
    // zero out bss
    zero_bss();

    // setup a temporary trap handler (just a busy loop)
    // so we can debug if there are errors
    unsafe {
        asm!(
            "lla s4, {start_hang}",
            "csrw {csr_mtvec}, s4",

            // clear mdt
            "li t0, 0x0000040000000000",
            "csrc {csr_mstatus}, t0",
            start_hang = sym start_hang,
            csr_mtvec = const opensbi::CSR_MTVEC,
            csr_mstatus = const opensbi::CSR_MSTATUS,
        )
    }

    // fw_platform_init correctly configures the "platform" struct
    unsafe {
        asm!(
            // Save registers a0-a4 to s0-s4
            "add s0, a0, zero",
            "add s1, a1, zero",
            "add s2, a2, zero",
            "add s3, a3, zero",
            "add s4, a4, zero",

            // fw_platform_init returns the new device tree in a0
            // let's put it in a1
            "call {fw_platform_init}",

            // save a0 to t0 temporary
            "add t0, a0, zero",

            // Restore registers a0-a4 from s0-s4
            "add s0, a0, zero",
            "add s1, a1, zero",
            "add s2, a2, zero",
            "add s3, a3, zero",
            "add s4, a4, zero",

            // save the new device tree in a1
            "add a1, t0, zero",

            fw_platform_init = sym opensbi::fw_platform_init
        )
    }

    // init scratch space: populate sbi_scratch structure with correct data
    init_scratch_space();

    // initialize cove extension
    cove::init();

    // call sbi_init for the current hart
    _start_warm()
}

#[inline(always)]
// This functions simply loops over the bss section
// and puts everything to zero.
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

#[inline(always)]
// This function resets all general-purpose registers to zero and clears the machine scratch register.
// It ensures that all previous instructions have been completed before performing the reset.
fn reset_registers() {
    unsafe {
        asm!(
            // Ensure all previous instructions have been completed
            "fence.i",
            // Set general-purpose registers to zero
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
            // Clear the machine scratch register
            "csrw {csr_mscratch}, 0",
            csr_mscratch = const opensbi::CSR_MSCRATCH,
        )
    }
}

/// Setup scratch space for HART 0
///
/// This function initializes the scratch space for HART 0, which is a per-HART data structure
/// defined in <sbi/sbi_scratch.h>. The scratch space is used to store various firmware-related
/// parameters and configurations necessary for the operation of the RISC-V system.
///
/// The `sbi_scratch` structure includes the following fields:
/// - `fw_start`: The start address of the firmware.
/// - `fw_size`: The size of the firmware.
/// - `fw_rw_offset`: The offset to the read/write section of the firmware.
/// - `fw_heap_offset`: The offset to the heap section.
/// - `fw_heap_size`: The size of the heap section.
/// - `next_arg1`: The argument to be passed to the next stage.
/// - `next_addr`: The address of the next stage to jump to.
/// - `next_mode`: The mode in which the next stage should be executed.
/// - `warmboot_addr`: The address for warm booting.
/// - `platform_addr`: The address of the platform-specific data.
/// - `hartid_to_scratch`: The function address to calculate the scratch space for a given HART.
/// - `trap_context`: The context for handling traps.
/// - `tmp0`: A temporary storage field.
/// - `options`: Firmware options.
/// - `hartindex`: The index of the HART.
///
/// The memory layout of the firmware is as follows:
/// - Firmware Region: Contains the firmware code and data, including the R/W section.
/// - HART Stacks: Contains the stack space for all HARTs, with each stack having a scratch area.
/// - Heap Region: A contiguous block of memory for heap usage.
///
/// This function performs the following steps:
/// 1. Retrieves platform details such as HART count, stack size, and heap size.
/// 2. Sets up the scratch space for all HARTs by calculating the appropriate memory addresses.
/// 3. Initializes the heap base address.
/// 4. Configures the scratch space for HART 0 by storing various firmware parameters.
/// 5. Clears the trap context and temporary storage fields.
/// 6. Stores the firmware options and HART index in the scratch space.
///
/// * This structure describes the memory layout of the firmware:
// - *                 Memory Layout
// -                -------------
// -+---------------------------------------------------------+
// -| Firmware Region                                         |
// -|                                                         |
// -|  _fw_start                                              |
// -|    +-----------------------------------------------+    |
// -|    |   Firmware Code & Data                        |    |
// -|    |                                               |    |
// -|    |   (Includes the read/write (R/W) section,     |    |
// -|    |    beginning at _fw_rw_start)                   |    |
// -|    +-----------------------------------------------+    |
// -|  _fw_end                                                |
// -+---------------------------------------------------------+
// -| HART Stacks (for all HARTs, total size = s7 * s8)        |
// -|                                                         |
// -|  Hart 0 Stack:                                          |
// -|    +---------------------------+                        |
// -|    |  (Stack space)            |                        |
// -|    |                           |                        |
// -|    |  Scratch Area             | <-- SBI_SCRATCH_SIZE    |
// -|    |    (holds various fields: |    (e.g., fw_start,     |
// -|    |     fw_start, fw_size,     |     fw_size, RW offset,  |
// -|    |     fw_rw_offset,         |     heap offset/size,    |
// -|    |     heap offset/size,     |     boot parameters,     |
// -|    |     boot addresses, etc.) |     etc.)                |
// -|    +---------------------------+                        |
// -+---------------------------------------------------------+
// -| Heap Region                                             |
// -|  (Contiguous block of size s9)                          |
// -|                                                         |
// -+---------------------------------------------------------+
// -

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
/// Calculates the starting address of the scratch space for a given HART (Hardware Thread).
///
/// This function uses the HART ID and HART Index to determine the appropriate scratch space
/// starting address. It retrieves platform details such as the HART stack size and count,
/// and performs calculations to find the correct address.
///
/// # Safety
///
/// This function is unsafe because it directly manipulates machine-level registers and
/// relies on specific memory layout assumptions. It should only be called in a controlled
/// environment where these assumptions hold true.
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
            // get platform details
            "lla a4, {platform}",
            "lwu t0, {sbi_platform_hart_stack_size_offset}(a4)",
            "lwu t2, {sbi_platform_hart_count_offset}(a4)",

            // calculate the scratch starting address
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
/// Initializes the warm start process for the system. This function sets up the stack,
/// updates the machine scratch register, configures the trap handler, and initializes
/// the SBI (Supervisor Binary Interface) for the scratch space. It is responsible for
/// preparing the system to handle interrupts and manage the execution environment for
/// each hardware thread (HART).
///
/// # Safety
///
/// This function is unsafe because it directly manipulates machine-level registers and
/// relies on specific memory layout assumptions. It should only be called in a controlled
/// environment where these assumptions hold true.
fn _start_warm() -> ! {
    unsafe { asm!("li ra, 0") }
    reset_registers();

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
            options(noreturn)
        )
    }
}

#[no_mangle]
#[link_section = ".payload_dom0"]
/// The `kernel` function is the entry point for the kernel payload. It performs
/// two system calls (ecalls) to demonstrate interaction with the system's
/// supervisor binary interface (SBI). The function sends a message to the console
/// and then enters an infinite loop to halt further execution.
///
/// # Safety
///
/// This function is marked as unsafe because it involves direct interaction with
/// machine-level registers and relies on specific memory layout assumptions. It
/// should only be called in a controlled environment where these assumptions hold true.
fn kernel() {
    #[link_section = ".payload_dom0.tsm_info"]
    static TSM_INFO: cove::TsmInfo = cove::TsmInfo {
        tsm_state: cove::TsmState::TsmNotLoaded,
        tsm_impl_id: 0,
        tvm_vcpu_state_pages: 0,
        tvm_max_vcpus: 0,
        tvm_state_pages: 0,
        tsm_capabilities: 0,
        tsm_version: 0,
    };
    unsafe {
        asm!(

            // struct sbiret sbi_covh_get_tsm_info(unsigned long tsm_info_address, unsigned long tsm_info_len)
            "li a7, {coveh_ext_id}",
            "li a6, 0",
            "lla a0, {tsm_info_addr}",
            "li a1, {tsm_info_size}",
            "ecall",

            coveh_ext_id = const cove::COVEH_EXT_ID,
            tsm_info_addr = sym TSM_INFO,
            tsm_info_size = const core::mem::size_of::<cove::TsmInfo>(),
        );
    }
    loop {}
}

#[no_mangle]
#[link_section = ".payload_dom1"]
fn kernel_dom1() {
    static MSG: [u8; 32] = *b"Hello world shadowfax from dom1\n";
    unsafe {
        asm!(
            // First ecall: Send a message to the console
            "li a7, {extid1}",
            "li a6, {fid1}",
            "li a0, {len}",
            "lla a1, {msg}",
            "li a2, 0",
            "ecall",

            // Second ecall: Perform a custom operation defined by the COVH extension
            "li a7, {extid2}",
            "li a2, 0",
            "ecall",


            // Parameters for the ecalls
            extid1 = const 0x4442434E,
            fid1 = const 0x00,
            len = const MSG.len(),
            msg = sym MSG,

            extid2 = const cove::COVEH_EXT_ID,
        );
    }
    loop {}
}

#[inline(always)]
/// This function causes the processor to enter an infinite loop, effectively halting execution.
/// It is typically used as a placeholder or to indicate a state where further execution should not proceed.
fn start_hang() {
    loop {}
}

#[inline(always)]
/// Disables all interrupts by setting the machine interrupt-enable register (MIE) to zero.
///
/// # Safety
///
/// This function is unsafe because it directly manipulates the machine-level interrupt-enable register.
/// It should be used with caution as it affects the global interrupt state.
fn disable_interrupts() {
    unsafe { asm!("csrw {csr_mie}, zero", csr_mie = const opensbi::CSR_MIE ) }
}
