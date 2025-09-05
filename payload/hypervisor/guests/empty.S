        .global _start
        .section .text.entry, "ax", %progbits

_start:
        j loop

loop:   j loop
