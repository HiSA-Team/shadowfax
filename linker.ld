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

  . = ALIGN (0x1000);
  .text : {
    KEEP(*(.text._start));                  /* Ensure _start is placed first */
    KEEP(*(.text._trap_handler));
    *(.text*);                              /* Program code here */
  }

  /* PMP mandates R/W sections aligned in power of 2 */
  . = ALIGN(1 << LOG2CEIL(SIZEOF(.text)));

  PROVIDE(_fw_rw_start = .);
  .data : {
    *(.data*);
  }

  . = ALIGN(0x1000); /* Ensure next section is page aligned */

  .bss : {
    PROVIDE(_bss_start = .);
    *(.sbss);
    *(.sbss.*);
    *(.bss);
    *(.bss.*);
    . = ALIGN(8);
    PROVIDE(_bss_end = .);
  }

  /* End of the read-write data sections */
  . = ALIGN(0x1000); /* Need this to create proper sections */
  PROVIDE(_fw_end = .);

  /* Payload Untrusted Domain*/
  . = FW_UDOM_PAYLOAD_START;
  . = ALIGN(0x1000);
  .payload_udom : {
    PROVIDE(_payload_udom_start = .);
    *(.payload_udom);
    . = ALIGN(8);
    PROVIDE(_payload_udom_end = .);
  }
  . = ALIGN(0x1000);

  /* Payload Trusted Domain*/
  . = FW_TDOM_PAYLOAD_START;
  . = ALIGN(0x1000);
  .payload_tdom : {
    PROVIDE(_payload_tdom_start = .);
    KEEP(*(.payload_tdom));
    . = ALIGN(8);
    PROVIDE(_payload_tdom_end = .);
  }
}
