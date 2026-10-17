#include <stddef.h>

#include "baremetal.h"

/* Lives in confidential guest RAM; the host must not be able to read it. */
static volatile unsigned char tvm_secret[4096] __attribute__((aligned(4096)));

int main(void)
{
    for (size_t i = 0; i < sizeof(tvm_secret); ++i)
        tvm_secret[i] = 0xa5;

    puts("[TVM] secret planted; requesting shutdown\n");
    shutdown();
}
