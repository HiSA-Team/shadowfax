// clang-format off
// Author: Giuseppe Capasso
// Email: capassog97@gmail.com
// According to QEMU riscv general virt has a NS16550 UART
// https://www.qemu.org/docs/master/system/riscv/virt.html

#define UART_BASE 0x10000000

volatile unsigned char *uart = (volatile unsigned char * ) UART_BASE;

void uart_puts(const char *str) {
  while (*str) {
    *uart = *str++;
  }
}

void main() {
  char message[10];
  int a = 5;
  int b = 4;
  int c = a + b;

  message[0] = a + '0';
  message[1] = ' ';
  message[2] = '+';
  message[3] = ' ';
  message[4] = b + '0';
  message[5] = ' ';
  message[6] = '=';
  message[7] = ' ';
  message[8] = c + '0';
  message[9] = '\0';

  uart_puts("shadowfax says: ");
  uart_puts(message);
  uart_puts("\n");
  while (1);
}
