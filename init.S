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

#include <sbi/sbi_platform.h>
#include <sbi/sbi_scratch.h>

.section .text._start
.globl _start

/* A scratch is a per hart data structure reported below defined in <sbi/sbi_scratch.h>
struct sbi_scratch {
    unsigned long fw_start;
    unsigned long fw_size;
    unsigned long fw_rw_offset;
    unsigned long fw_heap_offset;
    unsigned long fw_heap_size;
    unsigned long next_arg1;
    unsigned long next_addr;
    unsigned long next_mode;
    unsigned long warmboot_addr;
    unsigned long platform_addr;
    unsigned long hartid_to_scratch;
    unsigned long trap_context;
    unsigned long tmp0;
    unsigned long options;
    unsigned long hartindex;
};

  In this example, to keep things minimal, we initialize only hart0 which is mandatory by the ISA specification.
  The goal is to use sbi_init to jump in M mode to the _main label.
 */

_start:
  call _main
  j _start_hang

_hartid_to_scratch:
	/*
	 * a0 -> HART ID (passed by caller)
	 * a1 -> HART Index (passed by caller)
	 * t0 -> HART Stack Size
	 * t1 -> HART Stack End
	 * t2 -> Temporary
	 */
	lla	t2, platform
#if __riscv_xlen > 32
	lwu	t0, SBI_PLATFORM_HART_STACK_SIZE_OFFSET(t2)
	lwu	t2, SBI_PLATFORM_HART_COUNT_OFFSET(t2)
#else
	lw	t0, SBI_PLATFORM_HART_STACK_SIZE_OFFSET(t2)
	lw	t2, SBI_PLATFORM_HART_COUNT_OFFSET(t2)
#endif
	sub	t2, t2, a1
	mul	t2, t2, t0
	lla	t1, _fw_end
	add	t1, t1, t2
	li	t2, SBI_SCRATCH_SIZE
	sub	a0, t1, t2
	ret

/* Map implicit memcpy() added by compiler to sbi_memcpy() */
	.section .text
	.align 3
	.globl memcpy
memcpy:
	tail	sbi_memcpy

	/* Map implicit memset() added by compiler to sbi_memset() */
	.section .text
	.align 3
	.globl memset
memset:
	tail	sbi_memset

	/* Map implicit memmove() added by compiler to sbi_memmove() */
	.section .text
	.align 3
	.globl memmove
memmove:
	tail	sbi_memmove

	/* Map implicit memcmp() added by compiler to sbi_memcmp() */
	.section .text
	.align 3
	.globl memcmp
memcmp:
	tail	sbi_memcmp

.globl _trap_handler
.weak _trap_handler
_trap_handler:
  j _start_hang

.globl _start_warm
_start_warm:
  j _start_hang

_start_hang:
  wfi
  j _start_hang

# Simple main function for testing purposes.
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
