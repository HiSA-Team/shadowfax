#include <stdint.h>

#include "baremetal.h"
#include "coordination.h"

extern unsigned char __confidential_guest_start[];

int main(void)
{
    struct coordination *coord = coordination_page();
    const uintptr_t base = (uintptr_t)__confidential_guest_start;
    const uintptr_t num_pages = GUEST_RAM_SIZE / 4096UL;

    puts("[HOST] Cross-domain reclaim test\n");
    require_ok("CONVERT_PAGES",
        covh_call(COVH_CONVERT_PAGES, base, num_pages, 0, 0, 0, 0));

    coord->base_address = base;
    coord->num_pages = num_pages;
    coord->status = PAGES_CONVERTED;
    coordination_fence();
    coord->magic = COORDINATION_MAGIC;
    coordination_fence();

    while (coord->status == PAGES_CONVERTED)
        __asm__ volatile("nop");
    if (coord->status != ATTACK_BLOCKED)
        fail("attacker reclaimed host-owned confidential pages", -1);

    require_ok("RECLAIM_PAGES",
        covh_call(COVH_RECLAIM_PAGES, base, num_pages, 0, 0, 0, 0));
    puts("[HOST] PASS: only the owner could reclaim its confidential pages\n");
    shutdown();
}
