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

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
extern "C" fn main() -> ! {
    loop {}
}
