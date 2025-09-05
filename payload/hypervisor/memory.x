ENTRY(entry)

/* Memory Layout:
 * FLASH (rx)  : 0x80A0_0000 - 0x819F_FFFF (16MB) - Code, rodata
 * NOX_RAM (rw): 0x81A0_0000 - 0x81DF_FFFF (4MB)  - Stack, heap
 * RAM   (rxw) : 0x81E0_0000 - 0x825F_FFFF (8MB)  - Data, bss
 * L2_LIM (rw) : 0x8260_0000 - 0x82AF_FFFF (4MB)  - Guest stacks
 */
MEMORY
{
  FLASH (rx)  : ORIGIN = 0x80A00000, LENGTH = 16M
  NOX_RAM (rw): ORIGIN = 0x81A00000, LENGTH = 4M
  RAM (rxw)   : ORIGIN = 0x81E00000, LENGTH = 8M
  L2_LIM (rw) : ORIGIN = 0x82600000, LENGTH = 4M
}

/* Region aliases for better readability */
REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_RODATA", FLASH);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);
REGION_ALIAS("REGION_HEAP", RAM);
REGION_ALIAS("REGION_STACK", RAM);
REGION_ALIAS("REGION_GUEST", L2_LIM);

_hv_boot_stack_size = 0x2000; /* 8k stack */
_hv_heap_size = 0x4000; /* 16k heap */
_g_stack_start = ORIGIN(L2_LIM) + LENGTH(L2_LIM);

SECTIONS {

    /* ===== CODE SECTION ===== */
    .text : ALIGN(4K) {
        /* Entry point must be first */
        KEEP(*(.text.entry));
        . = ALIGN(4K);
        *(.text .text.*);

        /* Ensure section ends on page boundary */
        . = ALIGN(4K);
    } > REGION_TEXT

    /* ===== READ-ONLY DATA ===== */
    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*);
        *(.srodata .srodata.*);
        . = ALIGN(4K);
    } > REGION_RODATA

    /* Guest kernel binary (if any) */
    .guest_kernel : ALIGN(4K) {
        _guest_kernel_start = .;
        KEEP(*(.guest_kernel));
        _guest_kernel_end = .;
        . = ALIGN(4K);
    } > REGION_RODATA

    /* ===== RAM SECTIONS ===== */

    /* Data section (initialized global/static variables) */
    .data : ALIGN(4K) {
        _sdata = .;
        *(.data .data.*);
        *(.sdata .sdata.*);
        . = ALIGN(4K);
        _edata = .;
    } > REGION_DATA AT > FLASH

    /* Hypervisor boot stack - placed first in RAM for easier debugging */
    .hv_boot_stack (NOLOAD) : ALIGN(4K) {
        _hv_boot_stack_start = .;
        . += _hv_boot_stack_size;
        _top_b_stack = .;
        _hv_boot_stack_end = .;
        . = ALIGN(4K);
    } > REGION_STACK

    /* Hypervisor heap */
    .hv_heap (NOLOAD) : ALIGN(4K) {
        _hv_heap_start = .;
        . += _hv_heap_size;
        _hv_heap_end = .;
        . = ALIGN(4K);
    } > REGION_HEAP

    /* BSS section (zero-initialized data) */
    .bss (NOLOAD) : ALIGN(4K) {
        _start_bss = .;
        *(.bss .bss.*);
        *(.sbss .sbss.*);
        *(COMMON);
        . = ALIGN(4K);
        _end_bss = .;
    } > REGION_BSS

    /* ===== MEMORY USAGE TRACKING ===== */
    _ram_start = ORIGIN(RAM);
    _ram_end = ORIGIN(RAM) + LENGTH(RAM);
    _ram_size = LENGTH(RAM);

    /* Calculate remaining RAM after all allocations */
    _ram_used = . - _ram_start;
    _ram_free = _ram_end - .;

    /* Guest memory region */
    _guest_memory_start = ORIGIN(L2_LIM);
    _guest_memory_end = ORIGIN(L2_LIM) + LENGTH(L2_LIM);
    _guest_memory_size = LENGTH(L2_LIM);

    /* ===== DISCARDED SECTIONS ===== */
    /DISCARD/ : {
        *(.eh_frame);
        *(.eh_frame_hdr);
        *(.note .note.*);
    }
}

/* ===== ASSERTIONS ===== */
/* Ensure we don't overflow RAM */
ASSERT(_ram_used <= _ram_size, "ERROR: RAM overflow detected!");

/* Ensure stack and heap don't overlap */
ASSERT(_hv_boot_stack_end <= _hv_heap_start, "ERROR: Boot stack overlaps with heap!");

/* Provide useful symbols for debugging */
PROVIDE(_flash_start = ORIGIN(FLASH));
PROVIDE(_flash_end = ORIGIN(FLASH) + LENGTH(FLASH));
PROVIDE(_flash_size = LENGTH(FLASH));
