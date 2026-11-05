#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"

/*
 * Convert GUEST_RAM_SIZE bytes of host memory to confidential memory with a
 * single COVH CONVERT_PAGES call and report how long it took. Shadowfax zeroes
 * the converted range inside that call, so with a fixed bookkeeping cost the
 * latency tracks the zeroize implementation the firmware was built with.
 */

#define PAGE_SIZE 4096UL
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE (2UL * 1024UL * 1024UL)
#endif

/*
 * Only the start is referenced: with a 4 GiB guest the region ends at 8 GiB,
 * out of a medany PC-relative reach from the launcher. launcher.ld already
 * asserts the region is exactly GUEST_RAM_SIZE long.
 */
extern unsigned char __confidential_guest_start[];

struct counters {
    uint64_t cycle;
    uint64_t instret;
    uint64_t time;
};

static struct counters read_counters(void)
{
    struct counters c;

    asm volatile("rdcycle %0" : "=r"(c.cycle));
    asm volatile("rdinstret %0" : "=r"(c.instret));
    asm volatile("rdtime %0" : "=r"(c.time));
    return c;
}

int main(void)
{
    uintptr_t guest_start = (uintptr_t)__confidential_guest_start;
    /* Keep GCC from folding the end into a `symbol + offset` relocation,
       which would again need a PC-relative reach past 2 GiB. */
    asm("" : "+r"(guest_start));
    uintptr_t guest_end = guest_start + GUEST_RAM_SIZE;
    struct counters before, after;
    struct sbiret ret;

    /* Dirty the range so the zeroize has real data to overwrite. */
    for (volatile uint64_t *p = (volatile uint64_t *)guest_start;
         (uintptr_t)p < guest_end; p += PAGE_SIZE / sizeof(*p))
        *p = 0xa5a5a5a5a5a5a5a5UL;

    puts("[HOST] Calling CONVERT_PAGES\n");
    before = read_counters();
    ret = covh_call(COVH_CONVERT_PAGES, guest_start,
                    GUEST_RAM_SIZE / PAGE_SIZE, 0, 0, 0, 0);
    after = read_counters();
    if (ret.error != 0)
        fail("CONVERT_PAGES", ret.error);

    puts("ZEROIZE,");
    putdec(GUEST_RAM_SIZE);
    putchar(',');
    putdec(after.cycle - before.cycle);
    putchar(',');
    putdec(after.instret - before.instret);
    putchar(',');
    putdec(after.time - before.time);
    putchar('\n');

    puts("[HOST] PASS: CONVERT_PAGES latency measured\n");
    shutdown();
}
