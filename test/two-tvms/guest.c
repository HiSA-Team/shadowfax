#include <stdint.h>

#include "baremetal.h"

int main(uintptr_t id)
{
    puts("[TVM] id ");
    puthex(id);
    puts("\n");

    shutdown();
}
