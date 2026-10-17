#ifndef SHADOWFAX_CROSS_DOMAIN_RECLAIM_COORDINATION_H
#define SHADOWFAX_CROSS_DOMAIN_RECLAIM_COORDINATION_H

#include <stdint.h>

#include "layout.h"

#define COORDINATION_ADDRESS SHADOWFAX_REGION_COORDINATION_MEMORY_BASE
#define COORDINATION_MAGIC   0x5245434c41494d53UL

enum test_status {
    TEST_WAITING = 0,
    PAGES_CONVERTED = 1,
    ATTACK_BLOCKED = 2,
    ATTACK_SUCCEEDED = 3,
};

struct coordination {
    volatile uintptr_t magic;
    volatile uintptr_t status;
    volatile uintptr_t base_address;
    volatile uintptr_t num_pages;
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
