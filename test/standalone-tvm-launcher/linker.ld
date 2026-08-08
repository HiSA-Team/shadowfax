OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = 0x8A000000, LENGTH = 96M
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
        . += 512K;
        __confidential_metadata_end = .;
    } > RAM

    .confidential_guest (NOLOAD) : ALIGN(64M) {
        __confidential_guest_start = .;
        . += 64M;
        __confidential_guest_end = .;
    } > RAM

    __image_end = .;

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
        *(.comment)
    }
}

ASSERT(__confidential_metadata_end <= 0x8BF00000,
       "standalone launcher overlaps the device tree")
ASSERT(__confidential_guest_start == 0x8C000000,
       "guest confidential RAM must start at 0x8C000000")
ASSERT(__confidential_guest_end <= ORIGIN(RAM) + LENGTH(RAM),
       "standalone launcher exceeds untrusted-domain RAM")
