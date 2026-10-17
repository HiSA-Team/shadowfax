#include <stdint.h>

#include "baremetal.h"
#include "coordination.h"

int main(void)
{
    struct coordination *coord = coordination_page();
    struct sbiret ret;

    while (coord->magic != COORDINATION_MAGIC ||
           coord->status != PAGES_CONVERTED)
        __asm__ volatile("nop");
    coordination_fence();

    puts("[ATTACKER] Reclaiming confidential pages converted by the host\n");
    ret = covh_call(COVH_RECLAIM_PAGES, coord->base_address,
                    coord->num_pages, 0, 0, 0, 0);
    if (ret.error != 0) {
        puts("[ATTACKER] PASS: reclaim of host-owned pages was rejected\n");
        coord->status = ATTACK_BLOCKED;
    } else {
        /* The stolen range is now part of the attacker's PMP map. */
        (void)*(volatile uintptr_t *)coord->base_address;
        puts("[ATTACKER] FAIL: stole host pages at ");
        puthex(coord->base_address);
        putchar('\n');
        coord->status = ATTACK_SUCCEEDED;
    }
    coordination_fence();
    halt();
}
