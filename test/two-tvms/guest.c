#include <stdint.h>

#include "baremetal.h"

int main(uintptr_t id)
{
    puts("[TVM] id ");
    puthex(id);
    puts("\n");

    (void)sbi_call(SBI_EXT_SRST, SBI_SRST_RESET,
                   0, 0, 0, 0, 0, 0);

    for (;;)
        asm volatile("wfi");
}
