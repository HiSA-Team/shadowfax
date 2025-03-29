#include <sbi/sbi_const.h>
#include <sbi/sbi_types.h>

#include <sbi/riscv_asm.h>
#include <sbi/riscv_encoding.h>
#include <sbi/riscv_atomic.h>
#include <sbi/riscv_barrier.h>
#include <sbi/riscv_dbtr.h>
#include <sbi/riscv_elf.h>
#include <sbi/riscv_fp.h>
#include <sbi/riscv_io.h>
#include <sbi/riscv_locks.h>

#include <sbi/fw_dynamic.h>

#include <sbi/sbi_bitmap.h>
#include <sbi/sbi_bitops.h>
#include <sbi/sbi_byteorder.h>
#include <sbi/sbi_console.h>
#include <sbi/sbi_const.h>
#include <sbi/sbi_cppc.h>
#include <sbi/sbi_csr_detect.h>
#include <sbi/sbi_dbtr.h>
#include <sbi/sbi_domain.h>
#include <sbi/sbi_domain_context.h>
#include <sbi/sbi_domain_data.h>
#include <sbi/sbi_ecall.h>
#include <sbi/sbi_ecall_interface.h>
#include <sbi/sbi_emulate_csr.h>
#include <sbi/sbi_error.h>
#include <sbi/sbi_fifo.h>
#include <sbi/sbi_fwft.h>
#include <sbi/sbi_hart.h>
#include <sbi/sbi_hartmask.h>
#include <sbi/sbi_heap.h>
#include <sbi/sbi_hfence.h>
#include <sbi/sbi_hsm.h>
#include <sbi/sbi_illegal_insn.h>
#include <sbi/sbi_init.h>
#include <sbi/sbi_ipi.h>
#include <sbi/sbi_irqchip.h>
#include <sbi/sbi_list.h>
#include <sbi/sbi_math.h>
#include <sbi/sbi_mpxy.h>
#include <sbi/sbi_platform.h>
#include <sbi/sbi_pmu.h>
#include <sbi/sbi_scratch.h>
#include <sbi/sbi_sse.h>
#include <sbi/sbi_string.h>
#include <sbi/sbi_system.h>
#include <sbi/sbi_timer.h>
#include <sbi/sbi_tlb.h>
#include <sbi/sbi_trap.h>
#include <sbi/sbi_trap_ldst.h>
#include <sbi/sbi_unit_test.h>
#include <sbi/sbi_unpriv.h>
#include <sbi/sbi_version.h>

extern struct sbi_platform platform;
extern unsigned long fw_platform_init(unsigned long arg0, unsigned long arg1,
                                      unsigned long arg2, unsigned long arg3,
                                      unsigned long arg4);
