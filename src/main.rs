/*
 * Shadowfax entry point. This codes initializes the platform and jumps to main function using **opensbi** as a
 * static library (https://github.com/riscv-software-src/opensbi/blob/master/docs/library_usage.md).
 * According to opensbi static library, there are two constraints required before calling init functions:
 * - The RISC-V MSCRATCH CSR must point to a valid OpenSBI scratch space (i.e. a struct sbi_scratch instance);
 * - The RISC-V SP register (i.e. the stack pointer) must be set per-HART pointing to distinct non-overlapping stacks.
 *
 * The external firmware or bootloader must ensure that interrupts are disabled in the MSTATUS and MIE CSRs
 * when calling the functions sbi_init() and sbi_trap_handler().
 *
 * Part of this code has been taken from https://github.com/riscv-software-src/opensbi/blob/master/firmware/fw_base.S
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
#![doc = include_str!("../README.md")]
#![no_std]
#![no_main]
#![feature(fn_align)]
#![feature(once_cell_get_mut)]
#![feature(naked_functions_rustic_abi)]

use core::{ffi, panic::PanicInfo};

use linked_list_allocator::LockedHeap;
use riscv::asm::wfi;

#[macro_use]
mod debug;
mod cove;

/// This module includes the `bindings.rs` generated
/// using `build.rs` which translates opensbi C definitions
/// in Rust. This could be also be included without the module,
/// but doing in this way mandates that every opensbi symbol
/// is used with `opensbi::<symbol>`.
mod opensbi {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(unused)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

mod shadowfax_core;
mod trap;

extern crate alloc;

#[global_allocator]
/// Global allocator.
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/*
 * This "object" is just to hold symbols declared in the linkerscript
 * In `linker.ld`, we define this values and this is a way to access them
 * from Rust.
 */
unsafe extern "C" {
    static _fw_start: u8;
    static _fw_end: u8;
    static _fw_rw_start: u8;
    static _start_bss: u8;
    static _end_bss: u8;
    static _top_b_stack: u8;
    static mut _tee_heap_start: u8;
    static _heap_size: u8;
    pub static _tee_scratch_start: u8;
}

/*
 * This is needed for rust bare metal programs
 */
#[inline(never)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    debug!("{}", info);
    loop {}
}

/// We include the `next-stage` .elf in the firmware as read-only data.
/// We cannot execute directly from here since we will have problems with non-executable
/// sections. The `load_elf` function will load this .elf in memory.
/// This technique "mocks" what happens when we pass the `-kernel` flag to QEMU.
/// However this will be more flexible since we will likely need to load more
/// payloads to support different domain.
///
// TODO: make the payload name variable
#[cfg(feature = "embed-elf")]
#[link_section = ".payload"]
static PAYLOAD: [u8; include_bytes!("../bin/payload.elf").len()] =
    *include_bytes!("../bin/payload.elf");

// Stack size per HART: 8K
const STACK_SIZE_PER_HART: usize = 4096 * 2;

/// The _start function is the first function loaded at the starting address of
/// the linkerscript. This function:
///
/// - setup a the stack pointer
/// - loads the custom device tree in `a1` register overwriting the default one
/// provided by qemu
/// - zero bss section
/// - call `fw_platform_init` provided by opensbi
/// - jump to main
/// temporary stack at the end of the firmware and jump to
/// main function.
/// Since qemu does not support creating opensbi domains
/// from the cli, we need to provide a custom linkerscript.
#[link_section = ".text.entry"]
#[no_mangle]
extern "C" fn start() -> ! {
    unsafe {
        core::arch::asm!(
            // If there are multiple hart, init only hartid 0
            "csrr s6, mhartid",
            // If not zero, go to wait loop
            "bnez s6, {hang}",

            // setup a temporary stack pointer
            "li t0, {stack_size_per_hart}",
            "mul t1, a0, t0",
            "la sp, {stack_top}",
            "sub sp, sp, t1",

            // zero out bss
            "la s4, {bss_start}",
            "la s5, {bss_end}",
            "0:",
            "sd zero, 0(s4)",
            "addi s4, s4, {pointer_size}",

            // Loop if s4 is less than s5
            "blt s4, s5, 0b",
            // call fw_platform_init
            // save registers a0-a4
            "add s0, a0, zero",
            "add s1, a1, zero",
            "add s2, a2, zero",
            "add s3, a3, zero",
            "add s4, a4, zero",
            "call {fw_platform_init}",
            // the platform init could change the device tree address
            // save the return value in t0
            "add t0, a0, zero",
            // restore the a0-a4 registers
            "add a0, s0, zero",
            "add a1, s1, zero",
            "add a2, s2, zero",
            "add a3, s3, zero",
            "add a4, s4, zero",
            // put the new fdt in a1
            "add a1, t0, zero",
            // Jump to our main function
            "call {main}",
            stack_size_per_hart = const STACK_SIZE_PER_HART,
            stack_top = sym _top_b_stack,
            hang = sym hang,
            fw_platform_init = sym opensbi::fw_platform_init,
            main = sym main,
            bss_start = sym _start_bss,
            bss_end = sym _end_bss,
            pointer_size = const size_of::<usize>(),
            options(noreturn)
        )
    }
}

#[allow(unused)]
enum PrivMode {
    PrivM = 3_isize,
    PrivS = 1,
    PrivU = 0,
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
#[link_section = ".text"]
extern "C" fn main(boot_hartid: usize, fdt_addr: usize) -> ! {
    unsafe {
        // Ensure all previous instructions have been completed
        riscv::asm::fence_i();
        core::arch::asm!(
            // remove ra
            "li ra, 0",
            // Set general-purpose registers to zero
            "li gp, 0",
            "li tp, 0",
            "li t0, 0",
            "li t1, 0",
            "li t2, 0",
            "li s0, 0",
            "li s1, 0",
            "li a0, 0",
            "li a1, 0",
            "li a2, 0",
            "li a3, 0",
            "li a4, 0",
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
        );
        riscv::register::mscratch::write(0);
    }
    // setup a temporary trap handler (just a busy loop)
    // so we can debug if there are errors
    unsafe {
        use riscv::register::mtvec::Mtvec;
        // set a temporary trap handler
        riscv::register::mtvec::write(Mtvec::from_bits(hang as usize));
    }
    // this enables heap allocations
    unsafe {
        // Initialize global allocator
        ALLOCATOR.lock().init(
            core::ptr::addr_of_mut!(_tee_heap_start),
            core::ptr::addr_of!(_heap_size) as usize,
        );
    }
    // prepare the next stage. Depending on the configuration,
    // we can include an elf and jump to the first section or
    // jump to a prefixed address.
    let next_stage_address = {
        #[cfg(feature = "embed-elf")]
        let next_stage_address = load_elf(&PAYLOAD);

        #[cfg(not(feature = "embed-elf"))]
        let next_stage_address = {
            let address = option_env!("SHADOWFAX_JUMP_ADDRESS")
                .unwrap_or("0x80A00000")
                .strip_prefix("0x")
                .unwrap();
            usize::from_str_radix(address, 16)
                .unwrap_or_else(|_| panic!("Invalid memory address: {}", address))
        };
        next_stage_address
    };

    // initialize shadowfax state which will be used to handle the CoVE SBI
    shadowfax_core::state::init(fdt_addr, next_stage_address).unwrap();

    /*
     * This code initializes the scratch space, which is a per-HART data structure
     * defined in <sbi/sbi_scratch.h>. The scratch space is used to store various firmware-related
     * parameters and configurations necessary for the operation of the RISC-V system.
     *
     * The memory layout of the firmware is as follows:
     * - Firmware Region: Contains the firmware code and data, including the R/W section.
     * - HART Stacks: Contains the stack space for all HARTs, with each stack having a scratch area.
     * - Heap Region: A contiguous block of memory for heap usage.
     *
     * This function performs the following steps:
     * 1. Retrieves platform details such as HART count, stack size, and heap size.
     * 2. Sets up the scratch space for all HARTs by calculating the appropriate memory addresses.
     * 3. Initializes the heap base address.
     * 4. Configures the scratch space for HART 0 by storing various firmware parameters.
     * 5. Clears the trap context and temporary storage fields.
     * 6. Stores the firmware options and HART index in the scratch space.
     *
     * This structure describes the memory layout of the firmware:
     * -                 Memory Layout
     * -                -------------
     * -+---------------------------------------------------------+
     * -| Firmware Region                                         |
     * -|                                                         |
     * -|  _fw_start                                              |
     * -|    +-----------------------------------------------+    |
     * -|    |   Firmware Code & Data                        |    |
     * -|    |                                               |    |
     * -|    |   (Includes the read/write (R/W) section,     |    |
     * -|    |    beginning at _fw_rw_start)                 |    |
     * -|    +-----------------------------------------------+    |
     * -|  _fw_end                                                |
     * -+---------------------------------------------------------+
     * -| HART Stacks (for all HARTs, total size = s7 * s8)       |
     * -|                                                         |
     * -|  Hart 0 Stack:                                          |
     * -|    +---------------------------+                        |
     * -|    |  (Stack space)            |                        |
     * -|    |                           |                        |
     * -|    |  Scratch Area             | <-- SBI_SCRATCH_SIZE   |
     * -|    |    (holds various fields: |    (e.g., fw_start,    |
     * -|    |     fw_start, fw_size,     |     fw_size, RW offset,  |
     * -|    |     fw_rw_offset,         |     heap offset/size,  |
     * -|    |     heap offset/size,     |     boot parameters,   |
     * -|    |     boot addresses, etc.) |     etc.)              |
     * -|    +---------------------------+                        |
     * -+---------------------------------------------------------+
     * -| Heap Region                                             |
     * -|  (Contiguous block of size s9)                          |
     * -|                                                         |
     * -+---------------------------------------------------------+
     */
    // Setup scratch space for all harts
    let hart_count = unsafe { opensbi::platform.hart_count } as usize;
    let hart_stack_size = unsafe { opensbi::platform.hart_stack_size } as usize;
    let heap_size = unsafe { opensbi::platform.heap_size } as usize;
    // parse linkerscript symbols
    let fw_start = unsafe { &_fw_start as *const u8 as usize };
    let fw_end = unsafe { &_fw_end as *const u8 as usize };
    let fw_rw_start = unsafe { &_fw_rw_start as *const u8 as usize };
    let platform_addr = &raw const opensbi::platform as *const _ as usize;

    /* From the fw_base.S of opensbi:
     *
     * /* Setup scratch space for all the HARTs */
     * lla	tp, _fw_end
     * mul	a5, s7, s8
     * add	tp, tp, a5
     * /* Setup heap base address */
     * lla	s10, _fw_start
     * sub	s10, tp, s10
     * add	tp, tp, s9
     * /* Keep a copy of tp */
     * add	t3, tp, zero
     *
     */
    let fw_end_tot = fw_end + (hart_count * hart_stack_size) + heap_size;
    let heap_start = fw_end + (hart_count * hart_stack_size) - fw_start;

    for i in 0..hart_count {
        /*
         * Populate the sbi_scratch struct with the correct values
         * We want to use ffi:c_ulong to avoid hardcoding pointer size.
         * This is needed if we use riscv32 architectures.
         * The rust bindgen library will generate the correct types
         * based on target architecture.
         */
        let sbi_scratch = opensbi::sbi_scratch {
            // fw_start: start address of the firmware
            fw_start: fw_start as ffi::c_ulong,
            // fw_size: total firmware size, includes harts' stack and heap
            fw_size: (fw_end_tot - fw_start) as ffi::c_ulong,
            // fw_rw_offset: offset where the data starts
            fw_rw_offset: (fw_rw_start - fw_start) as ffi::c_ulong,
            // fw_heap_offset: where the heap starts from fw_start
            fw_heap_offset: heap_start as ffi::c_ulong,
            // fw_heap_size: heap size specified by the platform
            fw_heap_size: heap_size as ffi::c_ulong,
            // next_arg1: the fdt_address passed to the next stage
            next_arg1: fdt_addr as ffi::c_ulong,
            // next_addr: address of the next stage
            next_addr: next_stage_address as ffi::c_ulong,
            // next_mode: mode used to launch next_addr
            next_mode: PrivMode::PrivS as ffi::c_ulong,
            // warmboot_addr: address of the warmboot function.
            // This is not supported for now, but is needed for
            // hotplug harts and multicore
            warmboot_addr: 0,
            // platform_addr: address of the opensbi::platform struct populated
            // with fw_platform_init
            platform_addr: platform_addr as ffi::c_ulong,
            // hartid_to_scratch: function used to retrieve the hart scratch given the id
            hartid_to_scratch: hartid_to_scratch as ffi::c_ulong,
            // trap_context: reset to 0
            trap_context: 0,
            // tmp0: reset to 0
            tmp0: 0,
            // options: to customize the sbi_runtime
            options: 0,
            // hartindex: current hart index 0-based.
            hartindex: i as ffi::c_ulong,
        };

        /*
         * Calculate the address where to write the scratch
         * add	tp, t3, zero
         * sub	tp, tp, s9
         * mul	a5, s8, t1
         * sub	tp, tp, a5
         * li	a5, SBI_SCRATCH_SIZE
         * sub	tp, tp, a5
         */
        let scratch_addr =
            fw_end_tot - heap_size - (hart_stack_size * i) - opensbi::SBI_SCRATCH_SIZE as usize;

        let p = scratch_addr as *mut opensbi::sbi_scratch;

        unsafe {
            // write the structure to the calculated address
            p.write_volatile(sbi_scratch);
        }
    }

    // Prepare and jump to sbi_init. We need to:
    //  - disable interrupts
    //  - find the scratch for hart 0
    unsafe {
        use riscv::register::mtvec::Mtvec;
        // According to the opensbi documentation, we need to disable the interrupt
        riscv::interrupt::disable();

        // Set the mscratch to the correct address
        let scratch_addr = hartid_to_scratch(boot_hartid, boot_hartid);
        riscv::register::mscratch::write(scratch_addr);

        // set the stack pointer to the scratch.
        // First thing they will need to do is to setup the stack pointer
        // to a valid location
        core::arch::asm!(
            "csrr a0, mscratch",
            "add tp, a0, zero",
            options(nomem, nostack)
        );

        // set the trap handler
        let a = Mtvec::from_bits(trap::handler as usize);
        riscv::register::mtvec::write(a);

        riscv::register::mstatus::clear_tsr();
        riscv::register::mstatus::clear_tvm();

        // call sbi_init for the current hart
        let sbi_scratch_addr = scratch_addr as *mut opensbi::sbi_scratch;
        core::arch::asm!(
            "add sp, tp, {}", in(reg) opensbi::SBI_SCRATCH_SIZE
        );
        opensbi::sbi_init(sbi_scratch_addr)
    }
}

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
#[link_section = ".text"]
extern "C" fn hartid_to_scratch(_hartid: usize, hartindex: usize) -> usize {
    // Number of harts, stack size & heap size from the OpenSBI platform struct:
    let hart_count = unsafe { opensbi::platform.hart_count as usize };
    let hart_stack_sz = unsafe { opensbi::platform.hart_stack_size as usize };
    let heap_sz = unsafe { opensbi::platform.heap_size as usize };

    // End of firmware code/data section:
    let fw_end = unsafe { &_fw_end as *const u8 as usize };

    // Total top-of-memory after firmware + all stacks + heap:
    let fw_end_tot = fw_end + hart_count * hart_stack_sz + heap_sz;

    // Compute exactly where you wrote the i-th hart’s scratch:
    let scratch_addr = fw_end_tot
        // back off the heap
        .saturating_sub(heap_sz)
        // back off earlier hart stacks
        .saturating_sub(hart_stack_sz * hartindex)
        // back off the scratch size itself
        .saturating_sub(opensbi::SBI_SCRATCH_SIZE as usize);

    scratch_addr
}

/// This functions loads an elf in memory and returns the entry address.
/// Loading an elf in memory means to load the LOAD segments.
///
/// Params:
///  - data: the slice of the included elf
///
///  Returns:
///  - the entry point address
#[cfg(feature = "embed-elf")]
#[link_section = ".text"]
fn load_elf(data: &[u8]) -> usize {
    use alloc::vec::Vec;
    use elf::{abi::PT_LOAD, endian::AnyEndian, segment::ProgramHeader, ElfBytes};

    let elf = ElfBytes::<AnyEndian>::minimal_parse(data).unwrap();
    let all_load_phdrs = elf
        .segments()
        .unwrap()
        .iter()
        .filter(|phdr| phdr.p_type == PT_LOAD)
        .collect::<Vec<ProgramHeader>>();

    for segment in all_load_phdrs {
        // Get segment details
        let p_offset = segment.p_offset as usize;
        let p_filesz = segment.p_filesz as usize;
        let p_paddr = segment.p_paddr as *mut u8;
        let p_memsz = segment.p_memsz as usize;
        // Check if the segment data is within bounds
        assert!(
            p_offset + p_filesz <= data.len(),
            "Segment data out of bounds"
        );

        // Copy the segment data to RAM
        let segment_data = &data[p_offset..p_offset + p_filesz];
        unsafe {
            core::ptr::copy_nonoverlapping(segment_data.as_ptr(), p_paddr, p_filesz);
        }
        // zero any .bss past the end of file
        if p_memsz > p_filesz {
            let bss_start = unsafe { p_paddr.add(p_filesz) };
            let bss_len = p_memsz - p_filesz;
            unsafe { core::ptr::write_bytes(bss_start, 0, bss_len) }
        }
    }

    // Return the entry point address of the ELF
    elf.ehdr.e_entry as usize
}

// Needed for opensbi
// For some reason the static lib needs these 2 symbols defined
// TODO: investigate why these are needed.
// Maybe we can just use libsbi.a (without libplatsbi.a) and provide the `fw_platform_init`
// externally.
#[no_mangle]
fn _start_warm() {}
#[no_mangle]
fn _trap_handler() {}

/// This function causes the processor to enter an infinite loop, effectively halting execution.
/// It is typically used as a placeholder or to indicate a state where further execution should not proceed.
#[rustc_align(4)]
fn hang() {
    loop {
        wfi()
    }
}
