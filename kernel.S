  .global main
  .section .text.kernel
main:
  li a7, 0x53555044
  li a6, 0x00
  ecall

loop:   j loop
