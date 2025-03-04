SECTIONS
{
    . = 0x80000000;
    .text : {
        *(.text)
		}
    . = ALIGN (CONSTANT (COMMONPAGESIZE));    
    .data : {
        *(.data)
		}
}
