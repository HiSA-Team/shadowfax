#include <stddef.h>

#define SBI_EXT_DBCN       0x4442434eUL
#define SBI_DBCN_WRITE_BYTE 2UL
#define SBI_EXT_SRST       0x53525354UL
#define SBI_SRST_RESET     0UL

struct sbiret {
    long error;
    long value;
};

static struct sbiret sbi_call(unsigned long eid, unsigned long fid,
                              unsigned long arg0, unsigned long arg1,
                              unsigned long arg2, unsigned long arg3,
                              unsigned long arg4, unsigned long arg5)
{
    register unsigned long a0 asm("a0") = arg0;
    register unsigned long a1 asm("a1") = arg1;
    register unsigned long a2 asm("a2") = arg2;
    register unsigned long a3 asm("a3") = arg3;
    register unsigned long a4 asm("a4") = arg4;
    register unsigned long a5 asm("a5") = arg5;
    register unsigned long a6 asm("a6") = fid;
    register unsigned long a7 asm("a7") = eid;

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
                   (unsigned long)(unsigned char)c, 0, 0, 0, 0, 0);
}

static void puts(const char *message)
{
    while (*message != '\0')
        putchar(*message++);
}

/* This is deliberately in guest RAM and must not survive reclaim. */
static volatile unsigned char tvm_secret[4096];

int main(void)
{
    for (size_t i = 0; i < sizeof(tvm_secret); ++i)
        tvm_secret[i] = 0xa5;

    puts("[TVM] secret planted; requesting shutdown\n");
    (void)sbi_call(SBI_EXT_SRST, SBI_SRST_RESET, 0, 0, 0, 0, 0, 0);

    for (;;)
        asm volatile("wfi");
}
