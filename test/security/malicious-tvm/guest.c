#include <stdint.h>
#include <stddef.h>

#include "baremetal.h"

#define SECRET_GPA 0x300000UL
#define SECRET_SIZE 4096UL

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
