/* Linkerscript used by shadowfax. It defines 3 main sections:
 *  - text: where firmware code is stored;
 *  - data: where r/w and bss is placed;
<<<<<<< HEAD
<<<<<<< HEAD
 *  - payload: a section where the payload lives
 *
 * Each section defines symbols to be used in the firmware to calculate offsets.
 * This linkerscript is parametric and it expects the linker to define required symbols
 * to work. Currently it expects:
 *  - FW_TEXT_START: the firware starting address;
 *  - FW_PAYLOAD_START: payload starting address;
 *
=======
 *  - payload: a section where the payload leaves
 *
 * Each section defines symbols to be used in the firmware to calculate offsets.
 * Currently the linkerscript works only on QEMU.
>>>>>>> 59c6cf7 (Working opensbi in Rust)
=======
 *  - payload: a section where the payload lives
 *
 * Each section defines symbols to be used in the firmware to calculate offsets.
 * This linkerscript is parametric and it expects the linker to define required symbols
 * to work. Currently it expects:
 *  - FW_TEXT_START: the firware starting address;
 *  - FW_PAYLOAD_START: payload starting address;
 *
>>>>>>> e3a4eb6 (Fix typo)
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

SECTIONS {
<<<<<<< HEAD
<<<<<<< HEAD
  . = FW_TEXT_START;
=======
  . = FW_TEXT_START;                           /* QEMU default load address to run bios */
>>>>>>> 1cdc447 (Parametrized linkerscript using build.rs)
=======
  . = FW_TEXT_START;
>>>>>>> 65cd049 (Better comments)
  PROVIDE(_fw_start = .);

<<<<<<< HEAD
<<<<<<< HEAD
  .text : ALIGN(4K) {
    *(.text.entry);
    *(.text*);
  }
  .rodata : ALIGN(4K) {
    *(.rodata.*);
<<<<<<< HEAD
    . = ALIGN(4K);
=======
  . = ALIGN (0x1000);
  .text : {
=======
  .text : ALIGN(4K) {
>>>>>>> 1344f98 (Including external payloads)
    *(.text.entry);
    *(.text*);
>>>>>>> 546d93d (Refactored code with pure (almost) Rust)
  }
  .rodata : ALIGN(4K) {
    *(.rodata .rodata.*);
=======
>>>>>>> 2c42367 (Working loading elf)
    . = ALIGN(4K);
  }

  /* PMP mandates R/W sections aligned in power of 2 */
  . = ALIGN(1 << LOG2CEIL(SIZEOF(.text) + SIZEOF(.rodata)));

  PROVIDE(_fw_rw_start = .);
  .data : ALIGN(4K) {
<<<<<<< HEAD
<<<<<<< HEAD
    *(.data.*);
=======
    *(.data .data.*);
>>>>>>> 1344f98 (Including external payloads)
=======
    *(.data.*);
>>>>>>> 2c42367 (Working loading elf)
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

<<<<<<< HEAD
<<<<<<< HEAD
  . = FW_TDOM_PAYLOAD_START;
  .payload : ALIGN(16K) {
    *(.payload);
    . = ALIGN(4K);
=======
  /* Payload Untrusted Domain*/
  . = FW_UDOM_PAYLOAD_START;
  . = ALIGN(0x1000);
  .payload_udom : {
    PROVIDE(_payload_udom_start = .);
    KEEP(*(.payload_udom.kernel_udom));
    *(.payload_udom*);
    . = ALIGN(8);
    /* This space is reserved for the stack of the payload */
    . += 8192;
    PROVIDE(_payload_udom_end = .);
>>>>>>> e9a75fd (Added missing semicolon to linker.ld)
  }

<<<<<<< HEAD
  /* This is for temporary stack at boot */
  . += 8192;

  PROVIDE(_top_b_stack = .);
=======
  . = ALIGN(0x1000);

<<<<<<< HEAD
  /* This is for temporary stack at boot */
  . += 8192;

  PROVIDE(_stack_top = .);

>>>>>>> 546d93d (Refactored code with pure (almost) Rust)
=======
  /* Payload Trusted Domain*/
  . = FW_TDOM_PAYLOAD_START;
  . = ALIGN(0x1000);
  .payload_tdom : {
    PROVIDE(_payload_tdom_start = .);
    KEEP(*(.payload_tdom));
    . = ALIGN(8);
    PROVIDE(_payload_tdom_end = .);
  }
>>>>>>> e9a75fd (Added missing semicolon to linker.ld)
=======
  /* Give the stack some space */
  . = FW_TDOM_PAYLOAD_START;
  .payload : ALIGN(16K) {
    *(.payload);
    . = ALIGN(4K);
  }

  /* This is for temporary stack at boot */
  . += 8192;

  PROVIDE(_top_b_stack = .);
>>>>>>> 1344f98 (Including external payloads)
}
