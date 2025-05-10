/* Linkerscript used by shadowfax. It defines 3 main sections:
 *  - text: where firmware code is stored;
 *  - data: where r/w and bss is placed;
 *  - payload: a section where the payload lives
 *
 * Each section defines symbols to be used in the firmware to calculate offsets.
 * This linkerscript is parametric and it expects the linker to define required symbols
 * to work. Currently it expects:
 *  - FW_TEXT_START: the firware starting address;
 *  - FW_PAYLOAD_START: payload starting address;
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

MEMORY
{
  FLASH (rx) : ORIGIN = 0x80000000, LENGTH = 4M
  BOOT_RAM (rw) : ORIGIN = 0x80400000, LENGTH = 256K
  RAM (rwx) : ORIGIN = 0x80500000, LENGTH = 20M
}

REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_RODATA", FLASH);
REGION_ALIAS("REGION_DATA", FLASH);
REGION_ALIAS("REGION_STACK", BOOT_RAM);

_stack_size = 0x4000;

SECTIONS {

  .text : ALIGN(4K) {
    _fw_start = .;
    *(.text.entry);
    . = ALIGN(4K);
    *(.text .text.*);
  } > REGION_TEXT

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
  . = ALIGN(1 << LOG2CEIL(SIZEOF(.text) + SIZEOF(.rodata)));
  _fw_rw_start = .;

  .data : ALIGN(4K) {
    *(.data .data.*);
    . = ALIGN(4K);
  } > REGION_DATA

  .bss : ALIGN(4K) {
    _start_bss = .;
    *(.sbss .sbss.*);
    *(.bss .bss.*);
    . = ALIGN(4K);
    _end_bss = .;
    _fw_end = .;
  } > REGION_DATA

  .boot_stack (NOLOAD): ALIGN(4K) {
    . += _stack_size;
    _top_b_stack = .;
  } > REGION_STACK
}
