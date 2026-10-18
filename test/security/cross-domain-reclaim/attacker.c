#include <stdint.h>

#include "baremetal.h"
#include "coordination.h"

/* Outside the host's guest RAM, so only ownership can reject it. */
#define ATTACKER_GPA 0x10000000UL

static void record(struct coordination *coord, enum attack attack,
                   const char *what, struct sbiret ret)
{
    if (ret.error != 0) {
        puts("[ATTACKER] PASS: ");
        puts(what);
        puts(" was rejected\n");
        coord->result[attack] = RESULT_BLOCKED;
    } else {
        puts("[ATTACKER] FAIL: ");
        puts(what);
        puts(" succeeded\n");
        coord->result[attack] = RESULT_SUCCEEDED;
    }
}

int main(void)
{
    struct coordination *coord = coordination_page();

    while (coord->magic != COORDINATION_MAGIC)
        __asm__ volatile("nop");

    /* Tampering: change the layout of a TVM another domain is building. */
    wait_for(coord, TVM_CREATED);
    record(coord, ATTACK_ADD_MEMORY_REGION, "adding a memory region to the host TVM",
           covh_call(COVH_ADD_MEMORY_REGION, coord->tvm_id, ATTACKER_GPA,
                     4096UL, 0, 0, 0));
    publish(coord, ATTACK_ADD_DONE);

    /* Denial of service: dispose of another domain's TVM. */
    wait_for(coord, TVM_STOPPED);
    record(coord, ATTACK_DESTROY_TVM, "destroying the host TVM",
           covh_call(COVH_DESTROY_TVM, coord->tvm_id, 0, 0, 0, 0, 0));
    publish(coord, ATTACK_DESTROY_DONE);

    /* Memory stealing: take back pages another domain converted. */
    wait_for(coord, TVM_DESTROYED);
    record(coord, ATTACK_RECLAIM_PAGES, "reclaiming the host pages",
           covh_call(COVH_RECLAIM_PAGES, coord->base_address,
                     coord->num_pages, 0, 0, 0, 0));
    if (coord->result[ATTACK_RECLAIM_PAGES] == RESULT_SUCCEEDED) {
        /* The stolen range is now part of the attacker's PMP map. */
        (void)*(volatile uintptr_t *)coord->base_address;
        puts("[ATTACKER] read stolen page at ");
        puthex(coord->base_address);
        putchar('\n');
    }
    publish(coord, ATTACK_RECLAIM_DONE);
    halt();
}
