SECTIONS
{
  /* The start does not matter since we are not using this as an ELF for now.*/
  . = 0x0;

  .text : {
    KEEP(*(._start))
  }
}
