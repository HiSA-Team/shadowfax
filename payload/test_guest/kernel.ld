ENTRY(_start)

SECTIONS
{
  /* Text goes at 0x8020 0000, the Linux-on-virt default */
  . = 0x80A00000;

  .text : {
    KEEP(*(.text.entry))    /* your entry code */
    *(.text*)               /* anything else in .text */
  }

  .rodata : {
    *(.rodata*)             /* your debug string */
  }
}
