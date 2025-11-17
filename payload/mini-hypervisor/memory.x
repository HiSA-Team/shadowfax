MEMORY
{
  FLASH (rx) : ORIGIN = 0x80800000, LENGTH = 1M
  BOOT_RAM (rw) : ORIGIN = 0x80900000, LENGTH = 10M
}

REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_RODATA", FLASH);
REGION_ALIAS("REGION_STACK", BOOT_RAM);

_b_stack_size = 0x2000;
_stack_start = ORIGIN(BOOT_RAM) + LENGTH(BOOT_RAM);

SECTIONS {

    .text : {
        KEEP(*(.text.entry));
        . = ALIGN(4K);
        *(.text .text.*);
    } > REGION_TEXT

    .boot_stack (NOLOAD) : ALIGN(4K) {
        _bottom_b_stack = .;
        . += _b_stack_size;
        _top_b_stack = .;
    } > REGION_STACK

    .guest_kernel : ALIGN(4K) {
        KEEP(*(.guest_kernel));
        . = ALIGN(4K);
    } > REGION_RODATA
}
