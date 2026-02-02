OUTPUT_ARCH(riscv)
ENTRY(_start)

PHDRS
{
	text PT_LOAD FLAGS(SHF_ALLOC | SHF_EXECINSTR);
}

SECTIONS
{
    . = 0x1000;

    /* 1. Initialization code and main text */
    .text : {
        KEEP(*(.text.init))   /* From benchmark script: startup code */
        *(.text .text.*)
    } : text

    /* 2. Host-Target Interface (HTIF) */
    /* Benchmarks use this to signal completion or print to console */
    . = ALIGN(0x40);
    .tohost : { *(.tohost) } : text

    /* 3. Read-only data */
    .rodata : ALIGN(8) {
        *(.rodata .rodata.*)
        *(.srodata.cst16) *(.srodata.cst8) *(.srodata.cst4) *(.srodata.cst2) *(.srodata*)
    } : text

    /* 4. Small Data and Global Pointer */
    /* RISC-V uses the 'gp' register to access this area quickly */
    .sdata : ALIGN(8) {
        __global_pointer$ = . + 0x800;
        *(.sdata .sdata.* .gnu.linkonce.s.*)
    } : text

    /* data segment */
    .data : { *(.data) }

    .sdata : {
      __global_pointer$ = . + 0x800;
      *(.srodata.cst16) *(.srodata.cst8) *(.srodata.cst4) *(.srodata.cst2) *(.srodata*)
      *(.sdata .sdata.* .gnu.linkonce.s.*)
    }

    /* bss segment */
    .sbss : {
      *(.sbss .sbss.* .gnu.linkonce.sb.*)
      *(.scommon)
    }
    .bss : { *(.bss) }

    /* thread-local data segment */
    .tdata :
    {
      _tdata_begin = .;
      *(.tdata)
      _tdata_end = .;
    }

    .tbss :
    {
      *(.tbss)
      _tbss_end = .;
    }

    _end = .;
}
