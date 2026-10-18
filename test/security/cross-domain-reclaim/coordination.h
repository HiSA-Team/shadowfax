#ifndef SHADOWFAX_CROSS_DOMAIN_RECLAIM_COORDINATION_H
#define SHADOWFAX_CROSS_DOMAIN_RECLAIM_COORDINATION_H

#include <stdint.h>

#include "layout.h"

#define COORDINATION_ADDRESS SHADOWFAX_REGION_COORDINATION_MEMORY_BASE
#define COORDINATION_MAGIC   0x5245434c41494d53UL

/* The host advances the TVM lifecycle; after each step the attacker strikes. */
enum test_status {
    TEST_WAITING = 0,
    TVM_CREATED,        /* host TVM is initializing */
    ATTACK_ADD_DONE,
    TVM_STOPPED,        /* host TVM ran and shut down */
    ATTACK_DESTROY_DONE,
    TVM_DESTROYED,      /* host destroyed its TVM; pages are free */
    ATTACK_RECLAIM_DONE,
};

enum attack {
    ATTACK_ADD_MEMORY_REGION = 0,
    ATTACK_DESTROY_TVM,
    ATTACK_RECLAIM_PAGES,
    ATTACK_COUNT,
};

enum attack_result {
    RESULT_PENDING = 0,
    RESULT_BLOCKED,
    RESULT_SUCCEEDED,
};

struct coordination {
    volatile uintptr_t magic;
    volatile uintptr_t status;
    volatile uintptr_t tvm_id;
    volatile uintptr_t base_address;
    volatile uintptr_t num_pages;
    volatile uintptr_t result[ATTACK_COUNT];
};

static inline void coordination_fence(void)
{
    __asm__ volatile("fence rw, rw" ::: "memory");
}

static inline struct coordination *coordination_page(void)
{
    return (struct coordination *)COORDINATION_ADDRESS;
}

static inline void publish(struct coordination *coord, uintptr_t status)
{
    coordination_fence();
    coord->status = status;
    coordination_fence();
}

static inline void wait_for(struct coordination *coord, uintptr_t status)
{
    while (coord->status != status)
        __asm__ volatile("nop");
    coordination_fence();
}

#endif
