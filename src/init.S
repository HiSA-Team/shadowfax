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
 * which is used to launch other firmware types with S or HS mode. Shadowfax will be a trusted
 * firmware, so it needs to run in M mode.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

.section .text._start
.global _start

_start:
    la sp, __stack_top    # Load the stack pointer
    add s0, sp, zero      # Set the frame pointer
    jal zero, _main       # Run main entry point - no argc
loop:
  wfi
  j loop                  # Spin forever in case main returns

.global _main
.weak _main
_main:
 	li a1, 0x10000000
  addi a0, x0, 0x68
	sb a0, (a1) # 'h'

	addi a0, x0, 0x65
	sb a0, (a1) # 'e'

	addi a0, x0, 0x6C
	sb a0, (a1) # 'l'

	addi a0, x0, 0x6C
	sb a0, (a1) # 'l'

	addi a0, x0, 0x6F
	sb a0, (a1) # 'o'

  ret

.section .data
.space 1024*8             # allocate 8K of memory to serve as initial stack
.align 16                 # Smallest stack allocation is 16 bytes, so align accordingly
__stack_top:              # The stack grows downward according the Risc-V ABI
