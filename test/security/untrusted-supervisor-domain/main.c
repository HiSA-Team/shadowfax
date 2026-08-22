#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"
#include "coordination.h"

#define PAGE_SIZE              4096UL
#define PAGE_DIRECTORY_SIZE    (16UL * PAGE_SIZE)
#define SEGMENT_STAGING_SIZE   (2UL * 1024UL * 1024UL)
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE         (16UL * 1024UL * 1024UL)
#endif

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

static uintptr_t create_tvm(void)
{
    const uintptr_t metadata = (uintptr_t)__confidential_metadata_start;
    const uintptr_t guest = (uintptr_t)__confidential_guest_start;
    uintptr_t create_params[2] __attribute__((aligned(16)));
    uintptr_t tvm_id;
    uintptr_t entry;
    const struct baremetal_elf_loader loader = {
        .elf = __guest_elf,
        .elf_size = (size_t)__guest_elf_size,
        .staging = segment_staging,
        .staging_size = sizeof(segment_staging),
        .physical_start = guest,
        .physical_end = (uintptr_t)__confidential_guest_end,
        .guest_ram_size = GUEST_RAM_SIZE,
        .premapped = 0,
        .require_ok = require_ok,
        .fail = fail,
    };

    clear_bytes(__confidential_metadata_start,
                (size_t)(__confidential_metadata_end -
                         __confidential_metadata_start));
    clear_bytes(__confidential_guest_start,
                (size_t)(__confidential_guest_end -
                         __confidential_guest_start));
    require_ok("CONVERT_METADATA",
        covh_call(COVH_CONVERT_PAGES, metadata,
                  ((uintptr_t)__confidential_metadata_end - metadata) / PAGE_SIZE,
                  0, 0, 0, 0));
    require_ok("CONVERT_GUEST",
        covh_call(COVH_CONVERT_PAGES, guest, GUEST_RAM_SIZE / PAGE_SIZE,
                  0, 0, 0, 0));

    create_params[0] = metadata;
    create_params[1] = metadata + PAGE_DIRECTORY_SIZE;
    tvm_id = (uintptr_t)require_ok("CREATE_TVM",
        covh_call(COVH_CREATE_TVM, (uintptr_t)create_params,
                  sizeof(create_params), 0, 0, 0, 0));
    require_ok("ADD_MEMORY_REGION",
        covh_call(COVH_ADD_MEMORY_REGION, tvm_id, 0, GUEST_RAM_SIZE,
                  0, 0, 0));
    entry = baremetal_load_guest_elf(tvm_id, &loader);
    require_ok("CREATE_VCPU",
        covh_call(COVH_CREATE_VCPU, tvm_id, 0, 0, 0, 0, 0));
    require_ok("FINALIZE_TVM",
        covh_call(COVH_FINALIZE_TVM, tvm_id, entry, 0, 0, 0, 0));
    return tvm_id;
}

int main(void)
{
    struct coordination *coord = coordination_page();
    uintptr_t tvm_id;

    coord->magic = COORDINATION_MAGIC;
    coord->status = ATTACK_WAITING;
    coordination_fence();
    puts("[HOST] Creating 16 MiB TVM\n");
    tvm_id = create_tvm();

    coord->confidential_address = (uintptr_t)__confidential_guest_start;
    coordination_fence();
    coord->status = TVM_READY;
    coordination_fence();
    while (coord->status == TVM_READY)
        __asm__ volatile("nop");
    if (coord->status != ATTACK_BLOCKED)
        fail("attacker accessed confidential memory", -1);

    puts("[HOST] Attacker was blocked by PMP\n");
    require_ok_silent("RUN_TVM",
        covh_call(COVH_RUN_VCPU, tvm_id, 0, 0, 0, 0, 0));
    puts("[HOST] TVM returned\n");
    puts("[HOST] PASS: untrusted supervisor could not read TVM memory\n");
    shutdown();
}
