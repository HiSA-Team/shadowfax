ENTRY(_start)

SECTIONS
{
  /* The start does not matteer since we are not using this as an ELF for now.*/
  . = 0x0;

  .text : {
    KEEP(*(.text.entry))
    *(.text*)
  }

  .rodata : {
    *(.rodata*)
  }
}
