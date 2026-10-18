#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"
#include "coordination.h"

#define PAGE_SIZE              4096UL
#define PAGE_DIRECTORY_SIZE    (64UL * PAGE_SIZE)
#define SEGMENT_STAGING_SIZE   (64UL * 1024UL)

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

static const char *const attack_names[ATTACK_COUNT] = {
    "add a memory region to the host TVM",
    "destroy the host TVM",
    "reclaim the host pages",
};

int main(void)
{
    struct coordination *coord = coordination_page();
    const uintptr_t metadata = (uintptr_t)__confidential_metadata_start;
    const uintptr_t metadata_pages =
        ((uintptr_t)__confidential_metadata_end - metadata) / PAGE_SIZE;
    const uintptr_t guest = (uintptr_t)__confidential_guest_start;
    const uintptr_t guest_pages = GUEST_RAM_SIZE / PAGE_SIZE;
    uintptr_t create_params[2] __attribute__((aligned(16)));
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
    uintptr_t tvm_id, entry;
    int stolen = 0;

    puts("[HOST] Cross-domain interference test\n");
    clear_bytes(__confidential_metadata_start, metadata_pages * PAGE_SIZE);
    clear_bytes(__confidential_guest_start, GUEST_RAM_SIZE);
    require_ok("CONVERT_META_PAGES",
        covh_call(COVH_CONVERT_PAGES, metadata, metadata_pages, 0, 0, 0, 0));
    require_ok("CONVERT_GUEST_PAGES",
        covh_call(COVH_CONVERT_PAGES, guest, guest_pages, 0, 0, 0, 0));

    create_params[0] = metadata;
    create_params[1] = metadata + PAGE_DIRECTORY_SIZE;
    tvm_id = (uintptr_t)require_ok("CREATE_TVM",
        covh_call(COVH_CREATE_TVM, (uintptr_t)create_params,
                  sizeof(create_params), 0, 0, 0, 0));

    coord->tvm_id = tvm_id;
    coord->base_address = guest;
    coord->num_pages = guest_pages;
    for (size_t i = 0; i < ATTACK_COUNT; ++i)
        coord->result[i] = RESULT_PENDING;
    coord->magic = COORDINATION_MAGIC;
    publish(coord, TVM_CREATED);
    wait_for(coord, ATTACK_ADD_DONE);

    require_ok("ADD_MEMORY_REGION",
        covh_call(COVH_ADD_MEMORY_REGION, tvm_id, 0, GUEST_RAM_SIZE, 0, 0, 0));
    entry = baremetal_load_guest_elf(tvm_id, &loader);
    require_ok("CREATE_VCPU",
        covh_call(COVH_CREATE_VCPU, tvm_id, 0, 0, 0, 0, 0));
    require_ok("FINALIZE_TVM",
        covh_call(COVH_FINALIZE_TVM, tvm_id, entry, 0, 0, 0, 0));
    (void)covh_call(COVH_RUN_VCPU, tvm_id, 0, 0, 0, 0, 0);
    puts("[HOST] TVM stopped\n");

    publish(coord, TVM_STOPPED);
    wait_for(coord, ATTACK_DESTROY_DONE);

    /* If the attacker destroyed the TVM, there is nothing left to destroy. */
    if (coord->result[ATTACK_DESTROY_TVM] != RESULT_SUCCEEDED)
        require_ok("DESTROY_TVM",
            covh_call(COVH_DESTROY_TVM, tvm_id, 0, 0, 0, 0, 0));

    publish(coord, TVM_DESTROYED);
    wait_for(coord, ATTACK_RECLAIM_DONE);

    require_ok("RECLAIM_META_PAGES",
        covh_call(COVH_RECLAIM_PAGES, metadata, metadata_pages, 0, 0, 0, 0));
    if (coord->result[ATTACK_RECLAIM_PAGES] != RESULT_SUCCEEDED)
        require_ok("RECLAIM_GUEST_PAGES",
            covh_call(COVH_RECLAIM_PAGES, guest, guest_pages, 0, 0, 0, 0));

    for (size_t i = 0; i < ATTACK_COUNT; ++i) {
        puts(coord->result[i] == RESULT_BLOCKED ? "[HOST] blocked: " : "[HOST] NOT blocked: ");
        puts(attack_names[i]);
        putchar('\n');
        stolen |= coord->result[i] != RESULT_BLOCKED;
    }
    if (stolen)
        fail("attacker operated on another domain's TVM or pages", -1);

    puts("[HOST] PASS: another domain could not touch the host TVM or its pages\n");
    shutdown();
}
