/*
 * TSM layout. build.rs defines TSM_STANDALONE (--defsym) for the `standalone`
 * feature; every other value follows from it here.
 *
 * Under Shadowfax the image is relocatable and its region must match the size
 * of the `tmem` region granted to this domain in the platform device tree:
 * Shadowfax derives the boot record and the runtime stack top from the DT
 * region, not from these symbols.
 *
 * Standalone, QEMU boots the TSM directly and loads the ELF at its link
 * address, so it is linked inside QEMU virt RAM, with room for the embedded
 * Linux guest (kernel, DTB, initramfs) and the lazy loader's heap.
 */
_tsm_region_start = DEFINED(TSM_STANDALONE) ? 0x90000000 : 0;
_tsm_region_size  = DEFINED(TSM_STANDALONE) ? 32M : 8M;
_heap_size        = DEFINED(TSM_STANDALONE) ? 16M : 1M;
_tsm_region_end   = _tsm_region_start + _tsm_region_size;

/*
 * The last 32K holds the boot record, which becomes the runtime stack.
 * Both the init stack and the runtime stack grow down from there with no
 * hardware guard, so reserve the space they actually need and refuse to link
 * an image that would let the stack reach .data/.got. The unoptimized DICE
 * context parse in `boot::initialize` measures just under 3 MiB.
 */
_tsm_stack_size = 3M;
_tsm_stack_top = _tsm_region_end - 32K;
_tsm_stack_bottom = _tsm_stack_top - _tsm_stack_size;

SECTIONS {
  . = _tsm_region_start;

  .text : ALIGN(4K) {
    KEEP(*(._start));
    *(.text .text.*);
    *(.rodata .rodata.*);
  }

  /*
   * Named so they follow .text: with no MEMORY region, ld would place these
   * orphans at the region start, ahead of _start, where the loader jumps.
   */
  .dynsym    : { *(.dynsym) }
  .gnu.hash  : { *(.gnu.hash) }
  .hash      : { *(.hash) }
  .dynstr    : { *(.dynstr) }
  .rela.dyn  : { *(.rela.dyn .rela.*) }
  .eh_frame  : { KEEP(*(.eh_frame)) }

  .data : ALIGN(8) {
    . = ALIGN(4K);
    *(.data .data.*);
    *(.sdata .sdata.*);
    _heap_start = .;
    . += _heap_size;
    _heap_end = .;
  }

  .bss : ALIGN(8) {
    __bss_start = .;
    *(.bss .bss.*);
    *(.sbss .sbss.*);
    __bss_end = .;
  }
}

/* With no MEMORY region, this is also what keeps the image inside its region. */
ASSERT(__bss_end <= _tsm_stack_bottom, "TSM image overlaps its stack")
