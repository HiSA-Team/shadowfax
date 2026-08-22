OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY { RAM (rwx) : ORIGIN = 0x94000000, LENGTH = 32M }

SECTIONS
{
    . = ORIGIN(RAM);
    .text : { KEEP(*(.text.init)) *(.text .text.*) } > RAM
    .rodata : ALIGN(16) { *(.rodata .rodata.*) } > RAM
    .data : ALIGN(16) { *(.data .data.*) *(.sdata .sdata.*) *(.got .got.*) } > RAM
    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.*) *(.sbss .sbss.*) *(COMMON)
        __bss_end = .;
    } > RAM
    .stack (NOLOAD) : ALIGN(16) { . += 16K; __stack_top = .; } > RAM
    /DISCARD/ : { *(.eh_frame .eh_frame.*) *(.comment) }
}
