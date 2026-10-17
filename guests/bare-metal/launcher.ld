OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = SHADOWFAX_RAM_BASE, LENGTH = SHADOWFAX_RAM_SIZE
    GUEST (rw) : ORIGIN = SHADOWFAX_GUEST_BASE, LENGTH = SHADOWFAX_GUEST_REGION_SIZE
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

    .confidential_metadata (NOLOAD) : ALIGN(SHADOWFAX_METADATA_SIZE) {
        __confidential_metadata_start = .;
        . += SHADOWFAX_METADATA_SIZE;
        __confidential_metadata_end = .;
    } > RAM

    .confidential_guest (NOLOAD) : {
        __confidential_guest_start = .;
        . += SHADOWFAX_GUEST_SIZE;
        __confidential_guest_end = .;
    } > GUEST

    __image_end = .;

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
        *(.comment)
    }
}

/* The launcher writes its own RAM and hands GUEST to the TSM, so the two must
   be disjoint. layout.py rejects overlapping DTS memregions, but nothing stops
   a launcher naming one region for both, and ld does not check MEMORY regions
   against each other. State it here rather than inferring it from the FDT
   asserts below. */
ASSERT(ORIGIN(GUEST) >= ORIGIN(RAM) + LENGTH(RAM) ||
       ORIGIN(RAM) >= ORIGIN(GUEST) + LENGTH(GUEST),
       "launcher RAM region and confidential guest region overlap")
ASSERT(__confidential_metadata_end <= SHADOWFAX_FDT_ADDR,
       "launcher overlaps the device tree")
ASSERT(SHADOWFAX_FDT_ADDR < SHADOWFAX_GUEST_BASE,
       "device tree must precede confidential guest RAM")
ASSERT(__confidential_guest_start == SHADOWFAX_GUEST_BASE,
       "guest confidential RAM starts at the wrong address")
ASSERT(__confidential_guest_end == SHADOWFAX_GUEST_BASE + SHADOWFAX_GUEST_SIZE,
       "guest confidential RAM has the wrong size")
ASSERT(SHADOWFAX_GUEST_UNIT_SIZE > 0 &&
       SHADOWFAX_GUEST_UNIT_SIZE % 4K == 0,
       "per-TVM guest RAM must be non-zero and page aligned")
ASSERT(SHADOWFAX_METADATA_SIZE > 0 &&
       (SHADOWFAX_METADATA_SIZE & (SHADOWFAX_METADATA_SIZE - 1)) == 0,
       "launcher metadata size must be a power of two")
