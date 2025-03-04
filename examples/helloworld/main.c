// clang-format off
#define UART_BASE 0x10000000

volatile unsigned char *uart = (volatile unsigned char * ) UART_BASE;

void uart_puts(const char *str) {
  while (*str) {       // Loop until value at string pointer is zero
    *uart = *str++; // Write character to transmitter register
  }
}

void main() {
  uart_puts("Hello World!\n"); // Write the string to the UART
  while (1); 
}
