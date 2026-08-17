OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = SHADOWFAX_RAM_BASE, LENGTH = SHADOWFAX_RAM_SIZE
}

SECTIONS
{
    . = ORIGIN(RAM);

    .text : {
        KEEP(*(.text.init))
        *(.text .text.*)
    } > RAM

    .rodata : ALIGN(16) {
        *(.rodata .rodata.*)
    } > RAM

    .data : ALIGN(16) {
        *(.data .data.*)
        *(.sdata .sdata.*)
        *(.got .got.*)
    } > RAM

    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        __bss_end = .;
    } > RAM

    .stack (NOLOAD) : ALIGN(16) {
        __stack_bottom = .;
        . += 16K;
        __stack_top = .;
    } > RAM

    .confidential_metadata (NOLOAD) : ALIGN(512K) {
        __confidential_metadata_start = .;
        . += 1M;
        __confidential_metadata_end = .;
    } > RAM

    .confidential_guest SHADOWFAX_GUEST_BASE (NOLOAD) : {
        __confidential_guest_start = .;
        . += SHADOWFAX_GUEST_SIZE;
        __confidential_guest_end = .;
    } > RAM

    __image_end = .;

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
        *(.comment)
    }
}

ASSERT(__confidential_metadata_end <= SHADOWFAX_HOST_FDT_ADDR,
       "launcher overlaps the device tree")
ASSERT(SHADOWFAX_HOST_FDT_ADDR < SHADOWFAX_GUEST_BASE,
       "device tree must precede confidential guest RAM")
ASSERT(__confidential_guest_start == SHADOWFAX_GUEST_BASE,
       "guest confidential RAM starts at the wrong address")
ASSERT(__confidential_guest_end == SHADOWFAX_GUEST_BASE + SHADOWFAX_GUEST_SIZE,
       "guest confidential RAM has the wrong size")
ASSERT(__confidential_guest_end <= ORIGIN(RAM) + LENGTH(RAM),
       "launcher exceeds untrusted-domain RAM")
