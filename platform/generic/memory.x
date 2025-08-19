/*
 * Linkerscript used by shadowfax. Memory is partitioned in 4 parts:
 *  - FLASH: where all code and read-only data (including payload and fdt) are stored
 *  - RAM_FW: where rw_data lives and firwmare are stored;
 *  - RAM_TEE: tsm-driver context
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

/*
 * FLASH    0x80000000 - 0x803FFFFF
 * RAM_FW   0x80400000 - 0x807FFFFF
 * RAM_TEE  0x80800000 - 0x8083FFFF
 */
MEMORY
{
  FLASH (rx) : ORIGIN = 0x80000000, LENGTH = 4M
  RAM_FW (rwx) : ORIGIN = 0x80400000, LENGTH = 4M
  RAM_TEE (rw) : ORIGIN = 0x80800000, LENGTH = 2M
}

/*
 * Memory regions alias to give semantic meaning to what we are storing.
 */
REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_RODATA", FLASH);
REGION_ALIAS("REGION_DATA", RAM_FW);
REGION_ALIAS("REGION_BOOT_STACK", RAM_TEE);
REGION_ALIAS("REGION_TEE_MEM", RAM_TEE);

/* variables */
_stack_size = 0x1000; /* 1k */
_heap_size = 0x80000; /* 512K */
_tee_stack_size = 0x100000; /* 1M */

_fw_start = ORIGIN(FLASH);

SECTIONS {

  /* text region */
  .text : ALIGN(4K) {
    *(.text.entry);
    . = ALIGN(4K);
    *(.text .text.*);
  } > REGION_TEXT

  /* read only data.*/
  .rodata : ALIGN(4K) {
    *(.rodata .rodata.*);
    . = ALIGN(4K);
  } > REGION_RODATA

  .dtb : ALIGN(4K) {
    *(.dtb);
    . = ALIGN(4K);
  } >  REGION_RODATA

  .payload : ALIGN(4K) {
    *(.payload .payload.*);
    . = ALIGN(4K);
  } > REGION_RODATA

  /* PMP mandates R/W sections aligned in power of 2 */
  . = ALIGN(1 << LOG2CEIL(SIZEOF(.text) + SIZEOF(.rodata) + SIZEOF(.dtb) + SIZEOF(.payload)));
  _fw_rw_start = .;

  /* here we can store heap data */
  .data : ALIGN(4K) {
    *(.data .data.*);
    . = ALIGN(4K);
  } > REGION_DATA

  /* store bss_data */
  .bss : ALIGN(4K) {
    _start_bss = .;
    *(.sbss .sbss.*);
    *(.bss .bss.*);
    . = ALIGN(4K);
    _end_bss = .;
    _fw_end = .;
  } > REGION_DATA

  /* Stack reserved for boot and initialization firmware pre sbi_init */
  .boot_stack (NOLOAD): ALIGN(4K) {
    . += _stack_size;
    _top_b_stack = .;
  } > REGION_BOOT_STACK

  .tee_ctx (NOLOAD): ALIGN(4K) {
    /* Heap used by tsm-driver */
    _tee_heap_start = .;
    . += _heap_size;

    /* Scratch memory for CoVE interrupt handling and interrupt handling*/
    . = ALIGN(4K);
    . += _tee_stack_size;
    _tee_scratch_start = .;
    } > REGION_TEE_MEM
}
