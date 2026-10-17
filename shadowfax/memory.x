MEMORY
{
  /* Shadowfax firmware memory: 0x80000000 - 0x803ffffff. */
  RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 4M
}

REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_RODATA", RAM);
REGION_ALIAS("REGION_DATA", RAM);

/* variables */
_stack_size          = 0x4000;   /* 16k */
_heap_size           = 0x10000;  /* 64k */

_fw_start  = ORIGIN(RAM);
_stack_top = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS {

  /* text region */
  .text : ALIGN(4K) {
    KEEP(*(._start));
    . = ALIGN(4K);
    *(.text .text.*);
    *(.entry);
  } > REGION_TEXT

  /* read only data.*/
  .rodata : ALIGN(4K) {
    *(.rodata .rodata.*);
    *(.eh_frame);
    . = ALIGN(4K);
  } > REGION_RODATA

  /* writable firmware data */
  .data ALIGN(1 << LOG2CEIL(SIZEOF(.text) + SIZEOF(.rodata))) : {
    _fw_rw_start = .;
    *(.data .data.*);
    *(.htif);
    . = ALIGN(4K);

    _heap_start = .;
    . += _heap_size;
    . = ALIGN(4K);
    _heap_end = .;
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

}
