OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = 0x8A000000, LENGTH = 16M
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

    .guest_elf : ALIGN(4K) {
        __guest_elf_start = .;
        KEEP(*(.guest_elf))
        __guest_elf_end = .;
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

    .confidential (NOLOAD) : ALIGN(16K) {
        __confidential_start = .;
        . += 4M;
        __confidential_end = .;
    } > RAM

    __image_end = .;

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
        *(.comment)
    }
}

ASSERT(__guest_elf_end > __guest_elf_start, "guest ELF is empty")
ASSERT(__confidential_end <= ORIGIN(RAM) + LENGTH(RAM),
       "standalone launcher exceeds untrusted-domain RAM")
