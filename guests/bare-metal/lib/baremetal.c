#include "baremetal.h"

struct sbiret sbi_call(uintptr_t eid, uintptr_t fid,
                       uintptr_t arg0, uintptr_t arg1,
                       uintptr_t arg2, uintptr_t arg3,
                       uintptr_t arg4, uintptr_t arg5)
{
    register uintptr_t a0 asm("a0") = arg0;
    register uintptr_t a1 asm("a1") = arg1;
    register uintptr_t a2 asm("a2") = arg2;
    register uintptr_t a3 asm("a3") = arg3;
    register uintptr_t a4 asm("a4") = arg4;
    register uintptr_t a5 asm("a5") = arg5;
    register uintptr_t a6 asm("a6") = fid;
    register uintptr_t a7 asm("a7") = eid;

    asm volatile("ecall"
                 : "+r"(a0), "+r"(a1)
                 : "r"(a2), "r"(a3), "r"(a4), "r"(a5),
                   "r"(a6), "r"(a7)
                 : "memory");

    return (struct sbiret){(long)a0, (long)a1};
}

struct sbiret covh_call(uintptr_t fid,
                        uintptr_t arg0, uintptr_t arg1,
                        uintptr_t arg2, uintptr_t arg3,
                        uintptr_t arg4, uintptr_t arg5)
{
    return covh_call_to(COVH_DEFAULT_TSM_DOMAIN, fid,
                        arg0, arg1, arg2, arg3, arg4, arg5);
}

struct sbiret covh_call_to(uintptr_t tsm_domain_id, uintptr_t fid,
                           uintptr_t arg0, uintptr_t arg1,
                           uintptr_t arg2, uintptr_t arg3,
                           uintptr_t arg4, uintptr_t arg5)
{
    uintptr_t targeted_fid =
        ((tsm_domain_id & COVH_DOMAIN_MASK) << COVH_DOMAIN_SHIFT) |
        (fid & 0xffffUL);

    return sbi_call(SBI_EXT_COVH, targeted_fid,
                    arg0, arg1, arg2, arg3, arg4, arg5);
}

void putchar(char character)
{
    (void)sbi_call(SBI_EXT_DBCN, SBI_DBCN_WRITE_BYTE,
                   (uintptr_t)(unsigned char)character, 0, 0, 0, 0, 0);
}

void puts(const char *message)
{
    while (*message != '\0')
        putchar(*message++);
}

void puthex(uintptr_t value)
{
    static const char digits[] = "0123456789abcdef";

    puts("0x");
    for (int shift = (int)(sizeof(value) * 8) - 4; shift >= 0; shift -= 4)
        putchar(digits[(value >> shift) & 0xf]);
}

void putdec(uint64_t value)
{
    char digits[20];
    size_t count = 0;

    if (value == 0) {
        putchar('0');
        return;
    }
    while (value != 0) {
        digits[count++] = (char)('0' + value % 10);
        value /= 10;
    }
    while (count != 0)
        putchar(digits[--count]);
}

void clear_bytes(void *address, size_t size)
{
    volatile unsigned char *destination = address;

    while (size-- != 0)
        *destination++ = 0;
}

void copy_bytes(void *destination, const void *source, size_t size)
{
    unsigned char *destination_bytes = destination;
    const unsigned char *source_bytes = source;

    while (size-- != 0)
        *destination_bytes++ = *source_bytes++;
}

uintptr_t align_down(uintptr_t value, size_t alignment)
{
    return value & ~((uintptr_t)alignment - 1);
}

uintptr_t align_up(uintptr_t value, size_t alignment)
{
    return (value + alignment - 1) & ~((uintptr_t)alignment - 1);
}

__attribute__((noreturn)) void halt(void)
{
    for (;;)
        asm volatile("wfi");
}

__attribute__((noreturn)) void shutdown(void)
{
    (void)sbi_call(SBI_EXT_SRST, SBI_SRST_RESET, 0, 0, 0, 0, 0, 0);
    halt();
}

__attribute__((noreturn)) void fail(const char *operation, long error)
{
    puts("[HOST] ERROR: ");
    puts(operation);
    puts(" returned ");
    puthex((uintptr_t)error);
    puts("\n");
    halt();
}

long require_ok(const char *operation, struct sbiret result)
{
    if (result.error != 0)
        fail(operation, result.error);

    puts("[HOST] ");
    puts(operation);
    puts(" OK\n");
    return result.value;
}

long require_ok_silent(const char *operation, struct sbiret result)
{
    if (result.error != 0)
        fail(operation, result.error);

    return result.value;
}
