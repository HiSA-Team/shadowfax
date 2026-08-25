#ifndef SHADOWFAX_TRUST_MAP_COORDINATION_H
#define SHADOWFAX_TRUST_MAP_COORDINATION_H

#include <stdint.h>

#define COORDINATION_ADDRESS 0x96000000UL
#define COORDINATION_MAGIC   0x54525553544d4150UL

enum test_status {
    TEST_WAITING = 0,
    TEST_RUNNING = 1,
    TEST_PASSED = 2,
    TEST_FAILED = 3,
};

struct coordination {
    volatile uintptr_t magic;
    volatile uintptr_t status;
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
