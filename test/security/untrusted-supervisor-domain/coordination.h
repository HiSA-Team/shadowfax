#ifndef SHADOWFAX_SECURITY_COORDINATION_H
#define SHADOWFAX_SECURITY_COORDINATION_H

#include <stdint.h>

#define COORDINATION_ADDRESS 0x96000000UL
#define COORDINATION_MAGIC   0x5345435552495459UL

enum attack_status {
    ATTACK_WAITING = 0,
    TVM_READY = 1,
    ATTACK_BLOCKED = 2,
    ATTACK_FAILED = 3,
};

struct coordination {
    volatile uintptr_t magic;
    volatile uintptr_t status;
    volatile uintptr_t confidential_address;
};

static inline void coordination_fence(void)
{
    __asm__ volatile("fence rw, rw" ::: "memory");
}

static inline struct coordination *coordination_page(void)
{
    return (struct coordination *)COORDINATION_ADDRESS;
}

#endif
