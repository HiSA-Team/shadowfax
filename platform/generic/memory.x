/*
 * Linkerscript used by shadowfax. Memory is partitioned in 4 parts:
 *  - FLASH: where all code and read-only data (including payload and fdt) are stored
 *  - RAM_FW: where rw_data lives and firwmare are stored;
 *  - BOOT_RAM: a section to store the stack of the boot code pre sbi_init.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

/* FLASH    0x80000000 - 0x803FFFFF
 * RAM_FW   0x80400000 - 0x807FFFFF
 * SHDFX_RAM 0x80800000 - 0x8083FFFF
 */
MEMORY
{
  FLASH (rx) : ORIGIN = 0x80000000, LENGTH = 4M
  RAM_FW (rwx) : ORIGIN = 0x80400000, LENGTH = 4M
  SHDFX_RAM (rw) : ORIGIN = 0x80800000, LENGTH = 1M
}

/*
 * Memory regions alias to give semantic meaning to what we are storing.
 */
REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_RODATA", FLASH);
REGION_ALIAS("REGION_DATA", RAM_FW);
REGION_ALIAS("REGION_BOOT_STACK", SHDFX_RAM);
REGION_ALIAS("REGION_SHDFX_HEAP", SHDFX_RAM);
REGION_ALIAS("REGION_TEE_STACK", SHDFX_RAM);

/* variables */
_stack_size = 0x1000; /* 1k */
_heap_size = 0x80000; /* 512K */
_tee_stack_size = 0x4000; /* 4k */

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

  /* Heap used by shadowfax */
  .shdfx_heap (NOLOAD): ALIGN(4K) {
    _shdfx_heap_start = .;
    . += _heap_size;
  } > REGION_SHDFX_HEAP

  /* Stack were to store TEE context */
  .tee_stack (NOLOAD): ALIGN(4k) {
    . += _tee_stack_size;
    _tee_stack_start = .;
  } > REGION_TEE_STACK
}
