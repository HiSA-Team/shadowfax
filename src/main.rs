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
#![doc = include_str!("../README.md")]
#![no_std]
#![no_main]
#![feature(fn_align)]
use core::{arch::asm, ffi, panic::PanicInfo};

use heapless::Vec;
use riscv::asm::wfi;

use cove::{SbiRet, TsmInfo};
use spin::mutex::SpinMutex;

mod cove;

/*
 * This module includes the `bindings.rs` generated
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

/*
 * This "object" is just to hold symbols declared in the linkerscript
 * In `linker.ld`, we define this values and this is a way to access them
 * from Rust.
 */
unsafe extern "C" {
    static _fw_start: u8;
    static _fw_end: u8;
    static _fw_rw_start: u8;
    static _bss_start: u8;
    static _bss_end: u8;
    static _stack_top: u8;
    static _payload_udom_end: u8;
}

/*
 * This is needed for rust bare metal programs
 */
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        wfi();
    }
}

// Stack size per HART: 8K
const STACK_SIZE_PER_HART: usize = 4096 * 2;

/*
 * The _start function is the first functionloaded at the
 * starting address of the linkerscript. Here we setup a
 * temporary stack at the end of the firmware and jump to
 * main function
 */
#[link_section = ".text.entry"]
#[no_mangle]
extern "C" fn start() -> ! {
    unsafe {
        asm!(
            // If there are multiple hart, init only hartid 0
            "csrr s6, mhartid",
            // If not zero, go to wait loop
            "bnez s6, {hang}",
            // setup a temporary stack pointer at the end of the firmware
            "li t0, {stack_size_per_hart}",
            "la sp, {stack_top}",
            // In RISCV stacks goes downward
            "add sp, sp, t0",
            // zero out bss
            // Load the address of _bss_start into s4
            "la s4, {bss_start}",
            // Load the address of _bss_end into s5
            "la s5, {bss_end}",
            "0:",
            // Store zero to the address pointed
            "sd zero, 0(s4)",
            // Increment s4 by the size of a double word (8 bytes)
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
            stack_top = sym _stack_top,
            hang = sym hang,
            fw_platform_init = sym opensbi::fw_platform_init,
            main = sym main,
            bss_start = sym _bss_start,
            bss_end = sym _bss_end,
            pointer_size = const size_of::<usize>(),
            options(noreturn)
        )
    }
}

enum PrivMode {
    PrivM = 3_isize,
    PrivS = 1,
    PrivU = 0,
}

/*
 * The main function serves as the entry point for the firmware execution. It performs
 * several critical initialization tasks to prepare the system for operation. These tasks
 * include zeroing out the BSS section, setting up a temporary trap handler, initializing
 * the platform, configuring the scratch space, and starting the warm boot process.
 *
 * # Safety
 *
 * This function is marked as unsafe because it involves direct manipulation of machine-level
 * registers and relies on specific memory layout assumptions. It should only be called in a
 * controlled environment where these assumptions hold true.
 */
#[link_section = ".text"]
extern "C" fn main(boot_hartid: usize, fdt_address: usize) -> ! {
    unsafe {
        // Ensure all previous instructions have been completed
        riscv::asm::fence_i();
        asm!(
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
    /* This code initializes the scratch space, which is a per-HART data structure
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
     * * This structure describes the memory layout of the firmware:
     * - *                 Memory Layout
     * -                -------------
     * -+---------------------------------------------------------+
     * -| Firmware Region                                         |
     * -|                                                         |
     * -|  _fw_start                                              |
     * -|    +-----------------------------------------------+    |
     * -|    |   Firmware Code & Data                        |    |
     * -|    |                                               |    |
     * -|    |   (Includes the read/write (R/W) section,     |    |
     * -|    |    beginning at _fw_rw_start)                   |    |
     * -|    +-----------------------------------------------+    |
     * -|  _fw_end                                                |
     * -+---------------------------------------------------------+
     * -| HART Stacks (for all HARTs, total size = s7 * s8)        |
     * -|                                                         |
     * -|  Hart 0 Stack:                                          |
     * -|    +---------------------------+                        |
     * -|    |  (Stack space)            |                        |
     * -|    |                           |                        |
     * -|    |  Scratch Area             | <-- SBI_SCRATCH_SIZE    |
     * -|    |    (holds various fields: |    (e.g., fw_start,     |
     * -|    |     fw_start, fw_size,     |     fw_size, RW offset,  |
     * -|    |     fw_rw_offset,         |     heap offset/size,    |
     * -|    |     heap offset/size,     |     boot parameters,     |
     * -|    |     boot addresses, etc.) |     etc.)                |
     * -|    +---------------------------+                        |
     * -+---------------------------------------------------------+
     * -| Heap Region                                             |
     * -|  (Contiguous block of size s9)                          |
     * -|                                                         |
     * -+---------------------------------------------------------+
     * -
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
            next_arg1: fdt_address as ffi::c_ulong,
            // next_addr: address of the next stage
            next_addr: kernel_udom as ffi::c_ulong,
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
            p.write(sbi_scratch);
        }
    }

    // initialize cove extension
    cove::init(fdt_address);

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
        asm!(
            "csrr a0, mscratch",
            "add tp, a0, zero",
            "add sp, tp, zero",
            options(nomem)
        );

        let scratch_addr = riscv::register::mscratch::read();

        // set the trap handler
        let a = Mtvec::from_bits(trap::_trap_handler as usize);
        riscv::register::mtvec::write(a);

        riscv::register::mstatus::clear_tsr();
        riscv::register::mstatus::clear_tvm();

        // call sbi_init for the current hart
        let sbi_scratch_addr = scratch_addr as *mut opensbi::sbi_scratch;
        opensbi::sbi_init(sbi_scratch_addr)
    }
}

/*
 * Calculates the starting address of the scratch space for a given HART (Hardware Thread).
 *
 * This function uses the HART ID and HART Index to determine the appropriate scratch space
 * starting address. It retrieves platform details such as the HART stack size and count,
 * and performs calculations to find the correct address.
 *
 * # Safety
 *
 * This function is unsafe because it directly manipulates machine-level registers and
 * relies on specific memory layout assumptions. It should only be called in a controlled
 * environment where these assumptions hold true.
 */
#[link_section = ".text"]
extern "C" fn hartid_to_scratch(_hartid: usize, hartindex: usize) -> usize {
    // temp variables
    let hart_count = unsafe { opensbi::platform.hart_count };
    let hart_stack_size = unsafe { opensbi::platform.hart_stack_size };
    let fw_end = unsafe { &_fw_end as *const u8 as usize };

    let target_hart = hart_count as usize - hartindex;
    let stack_offset = target_hart * hart_stack_size as usize;
    let scratch_end = fw_end + stack_offset;

    scratch_end - opensbi::SBI_SCRATCH_SIZE as usize
}

// Needed for opensbi
#[no_mangle]
fn _start_warm() {}

#[link_section = ".payload_udom"]
static DOMAINS: SpinMutex<Vec<cove::TsmInfo, 64>> = SpinMutex::new(Vec::new());

#[link_section = ".payload_udom"]
fn sbi_call(extid: usize, fid: usize, args: &[u64; 5]) -> SbiRet {
    let (error, value);
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") extid,
            in("a6") fid,
            inlateout("a0") args[0] => error,
            inlateout("a1") args[1] => value,
            in("a2") args[2],
            in("a3") args[3],
            in("a4") args[4],
        );
    }
    SbiRet { error, value }
}

#[link_section = ".payload_udom"]
static SBI_ARGS: SpinMutex<[u64; 5]> = SpinMutex::new([0, 0, 0, 0, 0]);

/* The `kernel_udom` function is the entry point for the untrusted kernel payload.
 * It performs ecalls to demonstrate interaction with the system's
 * supervisor binary interface (SBI). The function sends a message to the console
 * and then enters an infinite loop to halt further execution.
 *
 * For now, it uses SUPD extension to get active domains (sbi_supd_get_active_domains) and
 * for each one retrieves the state using sbi_ext_cove_host_get_tsm_info()
 *
 * # Safety
 *
 * This function is marked as unsafe because it involves direct interaction with
 * machine-level registers and relies on specific memory layout assumptions. It
 * should only be called in a controlled environment where these assumptions hold true.
 */
#[link_section = ".payload_udom"]
fn kernel_udom() {
    // set stack pointer
    unsafe {
        asm!(
            // Load the address of the stack variable into t0.
            "la   t0, {stack}",
            // Load the stack size (8K) into t1.
            "li   t1, {stack_size}",
            // Set the stack pointer: sp = t0 + t1.
            "add  sp, t0, t1",
            stack = sym _payload_udom_end,
            stack_size = const 4096 * 2,
        )
    }

    macro_rules! cove_pack_fid {
        ($sdid:expr, $fid:expr) => {
            (($sdid & 0x3F) << 26) | ($fid & 0xFFFF)
        };
    }

    // get all active_domains
    let args = SBI_ARGS.lock();
    let active_domains = sbi_call(
        cove::SUPD_EXT_ID as usize,
        cove::SBI_EXT_SUPD_GET_ACTIVE_DOMAINS as usize,
        &args,
    );

    // register active domains in our structure
    let domain_mask = active_domains.value;
    for i in 0..64 {
        if ((domain_mask >> i) & 0x01) == 1 {
            DOMAINS
                .lock()
                .insert(
                    i,
                    TsmInfo {
                        tsm_state: cove::TsmState::TsmLoaded,
                        tsm_impl_id: 0,
                        tsm_version: 0,
                        tsm_capabilities: 0,
                        tvm_state_pages: 0,
                        tvm_max_vcpus: 0,
                        tvm_vcpu_state_pages: 0,
                    },
                )
                .unwrap()
        }
    }

    for (i, domain) in DOMAINS.lock().iter_mut().enumerate() {
        let mut sbi_args = SBI_ARGS.lock();
        let fid = cove_pack_fid!(i, cove::SBI_EXT_COVE_HOST_GET_TSM_INFO as usize);
        sbi_args[0] = &raw const domain as u64;
        sbi_args[1] = size_of::<TsmInfo>() as u64;
        sbi_call(cove::COVEH_EXT_ID as usize, fid, &sbi_args);
    }

    loop {}
}

/*
 * This function causes the processor to enter an infinite loop, effectively halting execution.
 * It is typically used as a placeholder or to indicate a state where further execution should not proceed.
 */
#[repr(align(4))]
fn hang() {
    loop {
        wfi()
    }
}
