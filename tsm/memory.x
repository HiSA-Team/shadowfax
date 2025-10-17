MEMORY
{
    RAM (rwx) : ORIGIN = 0x81400000, LENGTH = 8M
}

REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);
REGION_ALIAS("REGION_HEAP", RAM);

__head
SECTIONS {
    . = ORIGIN(RAM);

    .text : ALIGN(4K) {
        KEEP(*(._start));
        *(.text .text.*);
        *(.rodata .rodata.*);
    } > REGION_TEXT

    .data : ALIGN(8) {
        *(.data .data.*);
        *(.sdata .sdata.*);
    } > RAM

    .bss : ALIGN(8) {
        __bss_start = .;
        *(.bss .bss.*);
        *(.sbss .sbss.*);
        __bss_end = .;
    } > REGION_BSS

    .stack (NOLOAD): ALIGN(4K) {
        /* 8K stack */
        . += 0x2000;
        _top_b_stack = .;
    } > REGION_DATA

    .heap : ALIGN(8) {
        __heap_start = .;
        . += 1 * 1024 * 1024
        __heap_end = .;
    } > REGION_HEAP
}
