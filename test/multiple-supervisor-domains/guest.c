#include <stdint.h>

#include "baremetal.h"

int main(uintptr_t trusted_domain_id)
{
    puts("[TVM] Hello world from trusted domain ");
    putdec(trusted_domain_id);
    puts("\n");
    shutdown();
}
