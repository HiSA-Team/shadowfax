#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"

#define PAGE_SIZE                  4096UL
#define PAGE_DIRECTORY_SIZE        (64UL * PAGE_SIZE)
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE             (64UL * 1024UL * 1024UL)
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
        .premapped = 0,
        .require_ok = require_ok_silent,
        .fail = fail,
    };

    return baremetal_load_guest_elf(tvm_id, &loader);
}

int main(void)
{
    uintptr_t metadata_start = (uintptr_t)__confidential_metadata_start;
    uintptr_t metadata_end = (uintptr_t)__confidential_metadata_end;
    uintptr_t guest_memory_start = (uintptr_t)__confidential_guest_start;
    uintptr_t guest_memory_end = (uintptr_t)__confidential_guest_end;
    uintptr_t page_table = metadata_start;
    uintptr_t tvm_state = page_table + PAGE_DIRECTORY_SIZE;
    uintptr_t guest_physical = guest_memory_start;
    uintptr_t create_params[2] __attribute__((aligned(16)));
    uintptr_t tvm_id;
    uintptr_t guest_entry;
    struct sbiret ret;

    puts("[HOST] TVM suspend/resume test\n");

    ret = sbi_call(SBI_EXT_SUPD, SBI_SUPD_GET_ACTIVE,
                   0, 0, 0, 0, 0, 0);
    require_ok_silent("GET_ACTIVE_DOMAINS", ret);
    if (((uintptr_t)ret.value & 0x3) != 0x3)
        fail("TSM domain is not active", -1);

    /* ADD_ZERO_PAGES maps pages without clearing them. */
    clear_bytes((void *)metadata_start, metadata_end - metadata_start);
    clear_bytes((void *)guest_memory_start, guest_memory_end - guest_memory_start);

    require_ok_silent("CONVERT_META_PAGES",
               covh_call(COVH_CONVERT_PAGES,
                         metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE,
                         0, 0, 0, 0));
    require_ok_silent("CONVERT_GUEST_PAGES",
               covh_call(COVH_CONVERT_PAGES,
                         guest_memory_start,
                         (guest_memory_end - guest_memory_start) / PAGE_SIZE,
                         0, 0, 0, 0));

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
                         tvm_id, guest_entry, 0, 0, 0, 0));

    puts("[HOST] run_tvm_vcpu: first call\n");
    require_ok_silent("RUN_VCPU_FIRST",
               covh_call(COVH_RUN_VCPU, tvm_id, 0, 0, 0, 0, 0));

    puts("[HOST] run_tvm_vcpu: second call\n");
    require_ok_silent("RUN_VCPU_SECOND",
               covh_call(COVH_RUN_VCPU, tvm_id, 0, 0, 0, 0, 0));

    require_ok_silent("DESTROY_TVM",
               covh_call(COVH_DESTROY_TVM, tvm_id, 0, 0, 0, 0, 0));
    require_ok_silent("RECLAIM_META_PAGES",
               covh_call(COVH_RECLAIM_PAGES,
                         metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE,
                         0, 0, 0, 0));
    require_ok_silent("RECLAIM_GUEST_PAGES",
               covh_call(COVH_RECLAIM_PAGES,
                         guest_memory_start,
                         (guest_memory_end - guest_memory_start) / PAGE_SIZE,
                         0, 0, 0, 0));

    puts("[HOST] PASS: TVM suspended and resumed\n");
    halt();
}
