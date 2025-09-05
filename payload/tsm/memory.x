MEMORY
{
  FLASH (rx) : ORIGIN = 0x0, LENGTH = 1M
  BOOT_RAM (rw) : ORIGIN = 0x00100000, LENGTH = 1M
}

REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_STACK", BOOT_RAM);

_b_stack_size = 0x2000;

SECTIONS {

    .text : {
        KEEP(*(.text.entry));
        . = ALIGN(4K);
        *(.text .text.*);
    } > REGION_TEXT

    .boot_stack (NOLOAD) : ALIGN(4K) {
        . += _b_stack_size;
        _top_b_stack = .;
    } > REGION_STACK
}
