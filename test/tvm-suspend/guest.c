#include <stddef.h>

#include "baremetal.h"

int main(void)
{
    puts("[TVM] startup\n");

    (void)sbi_call(SBI_EXT_HSM, SBI_HSM_HART_SUSPEND,
                   0, 0, 0, 0, 0, 0);

    puts("[TVM] resume\n");
    shutdown();
}
