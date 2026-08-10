#include <stddef.h>

/* Minimal SBI return struct */
struct sbiret {
    long error;
    long value;
};

/* SBI IDs */
#define SBI_EXT_DBCN  0x4442434E
#define SBI_EXT_DBCN_CONSOLE_WRITE_BYTE  2

static inline struct sbiret sbi_ecall(
    unsigned long arg0, unsigned long arg1,
    unsigned long arg2, unsigned long arg3,
    unsigned long arg4, unsigned long arg5,
    unsigned long fid,  unsigned long eid) {
    register unsigned long a0 asm("a0") = arg0;
    register unsigned long a1 asm("a1") = arg1;
    register unsigned long a2 asm("a2") = arg2;
    register unsigned long a3 asm("a3") = arg3;
    register unsigned long a4 asm("a4") = arg4;
    register unsigned long a5 asm("a5") = arg5;
    register unsigned long a6 asm("a6") = fid;
    register unsigned long a7 asm("a7") = eid;

    asm volatile (
        "ecall"
        : "+r"(a0), "+r"(a1)
        : "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a6), "r"(a7)
        : "memory"
    );

    struct sbiret ret = {
        .error = (long)a0,
        .value = (long)a1,
    };
    return ret;
}

static void putc(char c) {
    sbi_ecall(
        (unsigned long)c,
        0, 0, 0, 0, 0,
        SBI_EXT_DBCN_CONSOLE_WRITE_BYTE,
        SBI_EXT_DBCN
    );
}

int main (void) {
    const char *msg = "Hello from TVM (VS-mode)\n";
    while (*msg)
        putc(*msg++);

    return 0;
}
