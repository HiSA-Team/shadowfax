/*
 * Linkerscript used by shadowfax. Memory is partitioned in 2 parts:
 *  - FLASH: where all code and read-only data (including the TSM, signatures and key) are stored
 *  - RAM: where data, TSM context and state are stored;
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

/*
 * FLASH    0x80000000 - 0x80FFFFFF
 * RAM      0x81000000 - 0x813FFFFF
 */
MEMORY
{
  FLASH (rx) : ORIGIN = 0x80000000, LENGTH = 16M
  RAM   (rw) : ORIGIN = 0x81000000, LENGTH = 4M
}

/*
 * Memory regions alias to give semantic meaning to what we are storing.
 */
REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_RODATA", FLASH);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BOOT_STACK", RAM);
REGION_ALIAS("REGION_TEE_MEM", RAM);

/* variables */
_boot_stack_size     = 0x4000;   /* 16k */
_heap_size           = 0x10000;  /* 64k */
_tee_stack_size      = 0x10000;  /* 64k */
_tsm_state_size      = 0x10000;  /* 64k */

_fw_start = ORIGIN(FLASH);

SECTIONS {

  /* text region */
  .text : ALIGN(4K) {
    KEEP(*(._start));
    . = ALIGN(4K);
    *(.text .text.*);
  } > REGION_TEXT

  /* read only data.*/
  .rodata : ALIGN(4K) {
    *(.rodata .rodata.*);
    . = ALIGN(4K);
  } > REGION_RODATA

  /* PMP mandates R/W sections aligned in power of 2 */
  . = ALIGN(1 << LOG2CEIL(SIZEOF(.text) + SIZEOF(.rodata)));
  _fw_rw_start = .;

  /* here we can store heap data */
  .data : ALIGN(4K) {
    *(.data .data.*);
    . = ALIGN(4K);

    _heap_start = .;
    . += _heap_size;
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
    . += _boot_stack_size;
    _boot_stack_top = .;
  } > REGION_BOOT_STACK

  /* Place where to store TEE state and context */
  .tee_ram (NOLOAD): ALIGN(4K) {
    /* Scratch memory for CoVE interrupt handling and interrupt handling*/
    . = ALIGN(4K);
    . += _tee_stack_size;
    _tee_stack_top = .;

    /* Space for TSM state */
    . = ALIGN(4K);
    _tsm_state_start = .;
    . += _tsm_state_size;

  } > REGION_TEE_MEM

  /* Mark the end of the RAM. TSM-driver can start loading the TSM */
  . = ALIGN(16K);
  _ram_end = .;
}
