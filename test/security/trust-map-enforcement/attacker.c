#include <stdint.h>

#include "baremetal.h"
#include "coordination.h"

#define TSM_DOMAIN_ID        1UL
#define COVH_GET_TSM_INFO    0UL
#define PAGE_SIZE            4096UL

static unsigned char tsm_info_page[PAGE_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

static void attack_failed(struct coordination *coord, const char *reason)
{
    puts("[ATTACKER] FAIL: ");
    puts(reason);
    putchar('\n');
    coord->status = TEST_FAILED;
    coordination_fence();
    halt();
}

int main(void)
{
    struct coordination *coord = coordination_page();
    struct sbiret ret;
    uintptr_t visible_domains;

    while (coord->magic != COORDINATION_MAGIC ||
           coord->status != TEST_WAITING)
        __asm__ volatile("nop");

    coord->status = TEST_RUNNING;
    coordination_fence();

    puts("[ATTACKER] Enumerating visible supervisor domains\n");
    ret = sbi_call(SBI_EXT_SUPD, SBI_SUPD_GET_ACTIVE,
                   0, 0, 0, 0, 0, 0);
    if (ret.error != 0)
        attack_failed(coord, "SUPD enumeration returned an SBI error");

    visible_domains = (uintptr_t)ret.value;
    if (visible_domains != 0)
        attack_failed(coord, "SUPD enumeration disclosed a domain");
    puts("[ATTACKER] PASS: SUPD enumeration returned no domains\n");

    puts("[ATTACKER] Guessing TSM domain ID 1\n");
    ret = covh_call_to(TSM_DOMAIN_ID, COVH_GET_TSM_INFO,
                       (uintptr_t)tsm_info_page, sizeof(tsm_info_page),
                       0, 0, 0, 0);
    if (ret.error == 0)
        attack_failed(coord, "guessed TSM domain accepted the call");
    puts("[ATTACKER] PASS: guessed TSM call was rejected\n");

    coord->status = TEST_PASSED;
    coordination_fence();
    halt();
}
