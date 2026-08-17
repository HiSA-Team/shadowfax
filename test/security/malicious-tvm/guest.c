#include <stdint.h>
#include <stddef.h>

#define SBI_EXT_DBCN 0x4442434eUL
#define SBI_DBCN_WRITE_BYTE 2UL
#define SBI_EXT_HSM 0x48534dUL
#define SBI_HSM_HART_SUSPEND 3UL
#define SBI_EXT_SRST 0x53525354UL

#define SECRET_GPA 0x300000UL
#define SECRET_SIZE 4096UL

struct sbiret { long error; long value; };

static struct sbiret sbi_call(unsigned long eid, unsigned long fid,
                               unsigned long a0v, unsigned long a1v,
                               unsigned long a2v, unsigned long a3v,
                               unsigned long a4v, unsigned long a5v)
{
    register unsigned long a0 asm("a0") = a0v;
    register unsigned long a1 asm("a1") = a1v;
    register unsigned long a2 asm("a2") = a2v;
    register unsigned long a3 asm("a3") = a3v;
    register unsigned long a4 asm("a4") = a4v;
    register unsigned long a5 asm("a5") = a5v;
    register unsigned long a6 asm("a6") = fid;
    register unsigned long a7 asm("a7") = eid;
    asm volatile("ecall" : "+r"(a0), "+r"(a1)
                 : "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a6), "r"(a7)
                 : "memory");
    return (struct sbiret){(long)a0, (long)a1};
}

static void putchar(char c)
{
    (void)sbi_call(SBI_EXT_DBCN, SBI_DBCN_WRITE_BYTE,
                   (unsigned long)(unsigned char)c, 0, 0, 0, 0, 0);
}

static void puts(const char *s)
{
    while (*s) putchar(*s++);
}

static void puthex(uintptr_t value)
{
    static const char digits[] = "0123456789abcdef";
    puts("0x");
    for (int shift = (int)(sizeof(value) * 8) - 4; shift >= 0; shift -= 4)
        putchar(digits[(value >> shift) & 0xf]);
}

static void shutdown(void)
{
    (void)sbi_call(SBI_EXT_SRST, 0, 0, 0, 0, 0, 0, 0);
    for (;;) asm volatile("wfi");
}

int main(uintptr_t role)
{
    volatile unsigned char *secret = (volatile unsigned char *)SECRET_GPA;

    if (role == 1) {
        static volatile unsigned char resumed;

        if (!resumed) {
            for (size_t i = 0; i < SECRET_SIZE; ++i)
                secret[i] = 0xa5;
            resumed = 1;
            puts("[TRUSTED] secret initialized\n");
            (void)sbi_call(SBI_EXT_HSM, SBI_HSM_HART_SUSPEND,
                           0, 0, 0, 0, 0, 0);
        }

        puts("[TRUSTED] checking secret\n");
        for (size_t i = 0; i < SECRET_SIZE; ++i) {
            if (secret[i] != 0xa5) {
                puts("[TRUSTED] secret corrupted\n");
                shutdown();
            }
        }
        puts("[TRUSTED] secret intact\n");
    } else {
        for (size_t i = 0; i < SECRET_SIZE; ++i) {
            if (secret[i] == 0xa5) {
                puts("[MALICIOUS] secret leaked at ");
                puthex(SECRET_GPA + i);
                puts("\n");
                shutdown();
            }
        }
        puts("[MALICIOUS] target GPA is not readable\n");
    }

    shutdown();
}
