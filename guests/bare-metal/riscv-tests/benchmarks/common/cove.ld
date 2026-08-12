OUTPUT_ARCH("riscv")
ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS(7);
}

SECTIONS
{
  . = 0x200000;
  .text.init : { *(.text.init) } : text

  . = ALIGN(0x1000);
  .tohost : { *(.tohost) }

  . = ALIGN(0x1000);
  .text : { *(.text .text.*) } : text
  .rodata : { *(.rodata .rodata.*) } : text
  .data : { *(.data .data.*) } : text

  .sdata : {
    __global_pointer$ = . + 0x800;
    *(.srodata.cst16) *(.srodata.cst8) *(.srodata.cst4) *(.srodata.cst2)
    *(.srodata*) *(.sdata .sdata.* .gnu.linkonce.s.*)
  } : text

  .sbss : {
    *(.sbss .sbss.* .gnu.linkonce.sb.*)
    *(.scommon)
  } : text
  .bss : { *(.bss .bss.*) } : text

  .tdata : {
    _tdata_begin = .;
    *(.tdata)
    _tdata_end = .;
  } : text
  .tbss : {
    *(.tbss)
    _tbss_end = .;
  } : text

  _end = .;
}
