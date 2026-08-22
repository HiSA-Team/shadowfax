#include <stdint.h>
#include "baremetal.h"

int main(uintptr_t unused)
{
    (void)unused;
    puts("[TVM] Hello world after blocked supervisor attack\n");
    shutdown();
}
