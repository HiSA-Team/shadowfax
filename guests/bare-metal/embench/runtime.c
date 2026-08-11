#include <stddef.h>
#include <stdint.h>

static uint64_t start_cycle;
static uint64_t elapsed_cycles;

static inline uint64_t
read_cycle(void)
{
    uint64_t value;
    __asm__ volatile("rdcycle %0" : "=r"(value));
    return value;
}

static void
uart_putchar(char c)
{
    volatile uint8_t *uart = (volatile uint8_t *)EMBENCH_UART_ADDR;

    while ((uart[5] & 0x20) == 0) {
    }
    uart[0] = (uint8_t)c;
}

static void
uart_put_u64(uint64_t value)
{
    char digits[20];
    unsigned int count = 0;

    do {
        digits[count++] = (char)('0' + value % 10);
        value /= 10;
    } while (value != 0);

    while (count != 0)
        uart_putchar(digits[--count]);
}

void
__wrap_start_trigger(void)
{
    start_cycle = read_cycle();
}

void
__wrap_stop_trigger(void)
{
    elapsed_cycles = read_cycle() - start_cycle;
}

static void __attribute__((noreturn))
shutdown(void)
{
#ifdef EMBENCH_SMODE
    register uintptr_t a0 __asm__("a0") = 0;
    register uintptr_t a1 __asm__("a1") = 0;
    register uintptr_t a6 __asm__("a6") = 0;
    register uintptr_t a7 __asm__("a7") = 0x53525354;

    __asm__ volatile("ecall"
                     : "+r"(a0)
                     : "r"(a1), "r"(a6), "r"(a7)
                     : "memory");
#else
    *(volatile uint32_t *)EMBENCH_TEST_ADDR = 0x5555;
#endif

    for (;;) {
    }
}

void __attribute__((noreturn))
embench_finish(int status)
{
    const char marker[] = "EMBENCH_RESULT,";

    for (size_t i = 0; i < sizeof(marker) - 1; ++i)
        uart_putchar(marker[i]);
    uart_put_u64(elapsed_cycles);
    uart_putchar(',');
    uart_put_u64((uint64_t)(unsigned int)status);
    uart_putchar('\n');
    shutdown();
}
