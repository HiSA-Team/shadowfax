MEMORY
{
    RAM (rwx) : ORIGIN = 0, LENGTH = 8M
}

REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);

_heap_size          = 1024 * 1024;

/*
 * Must match the size of the `tmem` region granted to this domain in the
 * platform device tree: Shadowfax derives the boot record and the runtime
 * stack top from the DT region, not from these symbols.
 */
_tsm_region_start = ORIGIN(RAM);
_tsm_region_end = ORIGIN(RAM) + LENGTH(RAM);

/*
 * The last 32K holds the boot record, which becomes the runtime stack.
 * Both the init stack and the runtime stack grow down from there with no
 * hardware guard, so reserve the space they actually need and refuse to link
 * an image that would let the stack reach .data/.got. The unoptimized DICE
 * context parse in `boot::initialize` measures just under 3 MiB.
 */
_tsm_stack_size = 3M;
_tsm_stack_top = _tsm_region_end - 32K;
_tsm_stack_bottom = _tsm_stack_top - _tsm_stack_size;

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

ASSERT(__bss_end <= _tsm_stack_bottom, "TSM image overlaps its stack")
