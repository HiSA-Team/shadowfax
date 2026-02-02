OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    /* GPA default payload address */
    RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 0x20000
}

__stack_top = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
    . = ORIGIN(RAM);

    .text : {
        KEEP(*(.text._start))
        *(.text*)
    } > RAM

    .rodata : ALIGN(8) {
        *(.rodata*)
    } > RAM

    .data : {
        *(.data*)
    } > RAM

    .bss : ALIGN(8) {
      __bss_start = .;
        *(.bss*)
        *(COMMON)
      __bss_end = .;
    } > RAM

}
