#ifndef SHADOWFAX_TSM_H
#define SHADOWFAX_TSM_H

#include <stdint.h>

#define SHADOWFAX_TSM_BOOT_MAGIC UINT64_C(0x5348445754534d31)
#define SHADOWFAX_TSM_BOOT_ABI_VERSION UINT32_C(1)
#define SHADOWFAX_TSM_MEASUREMENT_SIZE 64U

/*
 * Version 1 boot record passed by Shadowfax to an authenticated TSM.
 * All addresses are physical addresses and remain valid only during
 * _secure_init(), unless the TSM copies the referenced data.
 */
struct shadowfax_tsm_boot_info {
    uint64_t magic;
    uint32_t abi_version;
    uint32_t struct_size;
    uint64_t domain_id;
    uint64_t load_base;
    uint8_t measurement[SHADOWFAX_TSM_MEASUREMENT_SIZE];
    uint64_t dice_context_addr;
    uint64_t dice_context_size;
};

_Static_assert(sizeof(struct shadowfax_tsm_boot_info) == 112,
               "unexpected Shadowfax TSM boot ABI layout");

/* The symbol must be retained in the ELF static symbol table. */
intptr_t _secure_init(uintptr_t boot_info_addr);

#endif
