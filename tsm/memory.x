MEMORY
{
    RAM (rwx) : ORIGIN = 0x82000000, LENGTH = 8M
}

REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);

_heap_size          = 0x1000;  /* 4k */
_stack_size         = 0x2000;   /* 8k */

_stack_top = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS {
  . = ORIGIN(RAM);

  .text : ALIGN(4K) {
    KEEP(*(._start));
    *(.text .text.*);
    *(.rodata .rodata.*);
  } > REGION_TEXT

  .data : ALIGN(8) {
    . = ALIGN(4K);
    *(.data .data.*);
    *(.sdata .sdata.*);
    _heap_start = .;
    . += _heap_size;
    _heap_end = .;
  } > REGION_DATA

  .bss : ALIGN(8) {
    __bss_start = .;
    *(.bss .bss.*);
    *(.sbss .sbss.*);
    __bss_end = .;
  } > REGION_BSS

}
