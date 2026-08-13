#include <stddef.h>
#include <stdint.h>

#define SBI_EXT_DBCN        0x4442434eUL
#define SBI_DBCN_WRITE_BYTE 2UL

struct sbiret {
    long error;
    long value;
};

extern int security_launcher_main(void);

static struct sbiret sbi_call(uintptr_t eid, uintptr_t fid,
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

static void putchar(char c)
{
    (void)sbi_call(SBI_EXT_DBCN, SBI_DBCN_WRITE_BYTE,
                   (uintptr_t)(unsigned char)c, 0, 0, 0, 0, 0);
}

static void puts(const char *message)
{
    while (*message != '\0')
        putchar(*message++);
}

static void puthex(uintptr_t value)
{
    static const char digits[] = "0123456789abcdef";

    puts("0x");
    for (int shift = (int)(sizeof(value) * 8) - 4; shift >= 0; shift -= 4)
        putchar(digits[(value >> shift) & 0xf]);
}

__attribute__((noreturn))
static void fail(void)
{
    puts("[ATTACK] FAIL: reclaimed memory was not zeroized\n");
    for (;;)
        asm volatile("wfi");
}

static void assert_zeroed(const char *name, uintptr_t start, uintptr_t end)
{
    volatile const unsigned char *memory =
        (volatile const unsigned char *)start;

    for (uintptr_t address = start; address < end; ++address) {
        unsigned char value = memory[address - start];

        if (value != 0) {
            puts("[ATTACK] leaked byte in ");
            puts(name);
            puts(" at ");
            puthex(address);
            puts(": ");
            puthex(value);
            putchar('\n');
            fail();
        }
    }

    puts("[ATTACK] ");
    puts(name);
    puts(" contains only zeroes\n");
}

/*
 * This models an untrusted domain attempting to recover TVM data after a
 * successful destroy-and-reclaim sequence.
 */
void post_reclaim_hook(uintptr_t metadata_start, uintptr_t metadata_end,
                       uintptr_t guest_start, uintptr_t guest_end)
{
    puts("[ATTACK] reading reclaimed TVM memory\n");
    assert_zeroed("metadata", metadata_start, metadata_end);
    assert_zeroed("guest RAM", guest_start, guest_end);
    puts("[ATTACK] PASS: no reclaimed TVM data is visible\n");
}

int main(void)
{
    return security_launcher_main();
}
