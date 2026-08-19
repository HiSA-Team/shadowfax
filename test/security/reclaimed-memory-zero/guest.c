#include <stddef.h>

#include "baremetal.h"

/* This is deliberately in guest RAM and must not survive reclaim. */
static volatile unsigned char tvm_secret[4096];

int main(void)
{
    for (size_t i = 0; i < sizeof(tvm_secret); ++i)
        tvm_secret[i] = 0xa5;

    puts("[TVM] secret planted; requesting shutdown\n");
    shutdown();
}
