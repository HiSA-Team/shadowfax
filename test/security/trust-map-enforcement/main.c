#include "baremetal.h"
#include "coordination.h"

int main(void)
{
    struct coordination *coord = coordination_page();

    puts("[HOST] Test TSM trust-map enforcement\n");
    coord->status = TEST_WAITING;
    coord->magic = COORDINATION_MAGIC;
    coordination_fence();

    while (coord->status == TEST_WAITING || coord->status == TEST_RUNNING)
        __asm__ volatile("nop");

    if (coord->status != TEST_PASSED)
        fail("untrusted domain bypassed the TSM trust map", -1);

    puts("[HOST] PASS: untrusted domain could not discover or call the TSM\n");
    shutdown();
}
