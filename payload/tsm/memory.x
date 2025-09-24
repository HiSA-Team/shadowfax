MEMORY
{
    /* Use a generic base address - will be relocated by ELF loader */
    RAM (rwx) : ORIGIN = 0x0, LENGTH = 1M
}

REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);

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

    /* Add relocation sections */
    .rela.text : { *(.rela.text*) }
    .rela.data : { *(.rela.data*) }
    .rela.bss : { *(.rela.bss*) }
}
