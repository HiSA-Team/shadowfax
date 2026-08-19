#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"

#define PAGE_SIZE                  4096UL
#define PAGE_DIRECTORY_SIZE        (16UL * PAGE_SIZE)
#define TVM_METADATA_SIZE          (16UL * PAGE_SIZE)
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE             (16UL * 1024UL * 1024UL)
#endif
#define SEGMENT_STAGING_SIZE       (2UL * 1024UL * 1024UL)

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

static uintptr_t load_guest_elf(uintptr_t tvm_id, uintptr_t next_physical)
{
    const struct baremetal_elf_loader loader = {
        .elf = __guest_elf,
        .elf_size = (size_t)__guest_elf_size,
        .staging = segment_staging,
        .staging_size = sizeof(segment_staging),
        .physical_start = next_physical,
        .physical_end = (uintptr_t)__confidential_guest_end,
        .guest_ram_size = GUEST_RAM_SIZE,
        .premapped = 1,
        .require_ok = require_ok_silent,
        .fail = fail,
    };

    return baremetal_load_guest_elf(tvm_id, &loader);
}

static uintptr_t create_tvm(uintptr_t page_table, uintptr_t tvm_state,
                            uintptr_t guest_physical, uintptr_t entry_arg,
                            uintptr_t *tvm_id_out)
{
    uintptr_t create_params[2] __attribute__((aligned(16)));
    uintptr_t tvm_id;
    uintptr_t guest_entry;

    create_params[0] = page_table;
    create_params[1] = tvm_state;
    tvm_id = (uintptr_t)require_ok_silent(
        "CREATE_TVM",
        covh_call(COVH_CREATE_TVM,
                  (uintptr_t)create_params, sizeof(create_params),
                  0, 0, 0, 0));

    require_ok_silent("ADD_MEMORY_REGION",
               covh_call(COVH_ADD_MEMORY_REGION,
                         tvm_id, 0, GUEST_RAM_SIZE, 0, 0, 0));
    guest_entry = load_guest_elf(tvm_id, guest_physical);

    require_ok_silent("CREATE_VCPU",
               covh_call(COVH_CREATE_VCPU, tvm_id, 0, 0, 0, 0, 0));
    require_ok_silent("FINALIZE_TVM",
               covh_call(COVH_FINALIZE_TVM,
                         tvm_id, guest_entry, entry_arg, 0, 0, 0));

    *tvm_id_out = tvm_id;
    return guest_entry;
}

int main(void)
{
    uintptr_t metadata_start = (uintptr_t)__confidential_metadata_start;
    uintptr_t metadata_end = (uintptr_t)__confidential_metadata_end;
    uintptr_t guest_start = (uintptr_t)__confidential_guest_start;
    uintptr_t guest_end = (uintptr_t)__confidential_guest_end;
    uintptr_t page_table1 = metadata_start;
    uintptr_t state1 = page_table1 + PAGE_DIRECTORY_SIZE;
    uintptr_t page_table2 = metadata_start + TVM_METADATA_SIZE / 2;
    uintptr_t state2 = page_table2 + PAGE_DIRECTORY_SIZE;
    uintptr_t guest1 = guest_start;
    uintptr_t guest2 = guest1 + GUEST_RAM_SIZE;
    uintptr_t tvm1;
    uintptr_t tvm2;
    struct sbiret ret;

    if (guest2 + GUEST_RAM_SIZE > guest_end ||
        state2 + PAGE_SIZE > metadata_end)
        fail("launcher memory layout is too small", -1);

    ret = sbi_call(SBI_EXT_SUPD, SBI_SUPD_GET_ACTIVE,
                   0, 0, 0, 0, 0, 0);
    require_ok_silent("GET_ACTIVE_DOMAINS", ret);
    if (((uintptr_t)ret.value & 0x3) != 0x3)
        fail("TSM domain is not active", -1);

    clear_bytes((void *)metadata_start, metadata_end - metadata_start);
    clear_bytes((void *)guest_start, guest_end - guest_start);

    require_ok_silent("CONVERT_META_1",
               covh_call(COVH_CONVERT_PAGES,
                         metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE,
                         0, 0, 0, 0));
    require_ok_silent("CONVERT_GUEST",
               covh_call(COVH_CONVERT_PAGES,
                         guest1, (2 * GUEST_RAM_SIZE) / PAGE_SIZE,
                         0, 0, 0, 0));

    create_tvm(page_table1, state1, guest1, 1, &tvm1);
    create_tvm(page_table2, state2, guest2, 2, &tvm2);

    puts("[HOST] running TVM 1\n");
    require_ok_silent("RUN_TVM_1",
               covh_call(COVH_RUN_VCPU, tvm1, 0, 0, 0, 0, 0));

    puts("[HOST] running TVM 2\n");
    require_ok_silent("RUN_TVM_2",
               covh_call(COVH_RUN_VCPU, tvm2, 0, 0, 0, 0, 0));

    require_ok_silent("DESTROY_TVM_1",
               covh_call(COVH_DESTROY_TVM, tvm1, 0, 0, 0, 0, 0));
    require_ok_silent("DESTROY_TVM_2",
               covh_call(COVH_DESTROY_TVM, tvm2, 0, 0, 0, 0, 0));

    require_ok_silent("RECLAIM_META",
               covh_call(COVH_RECLAIM_PAGES,
                         metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE,
                         0, 0, 0, 0));
    require_ok_silent("RECLAIM_GUEST",
               covh_call(COVH_RECLAIM_PAGES,
                         guest1, (2 * GUEST_RAM_SIZE) / PAGE_SIZE,
                         0, 0, 0, 0));

    puts("[HOST] PASS: two TVMs completed\n");
    halt();
}
