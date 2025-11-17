.global _start
.attribute arch, "rv64imac"

_start:
  j loop

loop:   j loop
