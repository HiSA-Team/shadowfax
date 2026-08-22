#include <stdint.h>
#include "baremetal.h"
#include "coordination.h"

extern void attacker_trap(void);
extern uintptr_t attack_load(uintptr_t address);
volatile uintptr_t attack_faulted;

int main(void)
{
    struct coordination *coord = coordination_page();

    __asm__ volatile("csrw stvec, %0" :: "r"(attacker_trap) : "memory");
    while (coord->magic != COORDINATION_MAGIC || coord->status != TVM_READY)
        __asm__ volatile("nop");
    coordination_fence();

    puts("[ATTACKER] Reading confidential TVM memory\n");
    attack_faulted = 0;
    (void)attack_load(coord->confidential_address);
    coordination_fence();
    if (attack_faulted == 1) {
        puts("[ATTACKER] PASS: confidential read raised an access fault\n");
        coord->status = ATTACK_BLOCKED;
    } else {
        puts("[ATTACKER] FAIL: confidential memory was readable\n");
        coord->status = ATTACK_FAILED;
    }
    coordination_fence();
    halt();
}
