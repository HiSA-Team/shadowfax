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

SECTIONS {
  . = FW_TEXT_START;
  PROVIDE(_fw_start = .);

  .text : ALIGN(4K) {
    *(.text.entry);
    *(.text*);
  }
  .rodata : ALIGN(4K) {
    *(.rodata .rodata.*);
    . = ALIGN(4K);
  }

  /* PMP mandates R/W sections aligned in power of 2 */
  . = ALIGN(1 << LOG2CEIL(SIZEOF(.text) + SIZEOF(.rodata)));

  PROVIDE(_fw_rw_start = .);
  .data : ALIGN(4K) {
    *(.data .data.*);
    . = ALIGN(4K);
  }

  .bss : ALIGN(4K) {
    PROVIDE(_start_bss = .);
    *(.sbss);
    *(.sbss.*);
    *(.bss);
    *(.bss.*);
    . = ALIGN(4K);
    PROVIDE(_end_bss = .);
  }

  /* End of the read-write data sections */
  PROVIDE(_fw_end = .);

  /* Give the stack some space */
  . = FW_TDOM_PAYLOAD_START;

  /* This is for temporary stack at boot */
  . += 8192;

  PROVIDE(_top_b_stack = .);
}
