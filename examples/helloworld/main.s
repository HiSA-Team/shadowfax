.equ UART_BASE, 0x10000000

.section .text
  la a0, helloworld   # Load address of string
  li a1, UART_BASE    # Load uart tx base address
  call puts           # Print string

loop:	j loop          # End program; spin forever

puts:
  # a0 - String address
  # a1 - UART base address
1:                    # While string byte is not null
  lb t0, 0(a0)        # Get byte at current string pos
  beq zero, t0, 2f    # Is null?
  sb t0, (a1)         # No, write byte to port
  addi a0, a0, 1      # Inc string pos
  j 1b                # Loop
2:                    # String byte is null
  ret



.section .data
helloworld: .string "Hello World!"
