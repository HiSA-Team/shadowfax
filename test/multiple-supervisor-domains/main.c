#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"

#define PAGE_SIZE                  4096UL
#define PAGE_DIRECTORY_SIZE        (16UL * PAGE_SIZE)
#define PER_TVM_METADATA_SIZE      (512UL * 1024UL)
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE             (4UL * 1024UL * 1024UL)
#endif
#define SEGMENT_STAGING_SIZE       (2UL * 1024UL * 1024UL)

#define PRIMARY_TSM_DOMAIN         1UL
#define UNTRUSTED_DOMAIN           2UL
#define SECONDARY_TSM_DOMAIN       3UL
#define DOMAIN_BIT(id)             (1UL << (id))

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

static uintptr_t load_guest(uintptr_t tsm_domain_id, uintptr_t tvm_id,
                            uintptr_t physical_start)
{
    const struct baremetal_elf_loader loader = {
        .elf = __guest_elf,
        .elf_size = (size_t)__guest_elf_size,
        .staging = segment_staging,
        .staging_size = sizeof(segment_staging),
        .physical_start = physical_start,
        .physical_end = physical_start + GUEST_RAM_SIZE,
        .guest_ram_size = GUEST_RAM_SIZE,
        .premapped = 1,
        .require_ok = require_ok_silent,
        .fail = fail,
    };

    return baremetal_load_guest_elf_to(tsm_domain_id, tvm_id, &loader);
}

static uintptr_t create_tvm(uintptr_t tsm_domain_id,
                            uintptr_t metadata_start,
                            uintptr_t guest_start)
{
    uintptr_t create_params[2] __attribute__((aligned(16)));
    uintptr_t tvm_id;
    uintptr_t guest_entry;

    require_ok_silent(
        "CONVERT_METADATA",
        covh_call_to(tsm_domain_id, COVH_CONVERT_PAGES,
                      metadata_start, PER_TVM_METADATA_SIZE / PAGE_SIZE,
                      0, 0, 0, 0));
    require_ok_silent(
        "CONVERT_GUEST",
        covh_call_to(tsm_domain_id, COVH_CONVERT_PAGES,
                      guest_start, GUEST_RAM_SIZE / PAGE_SIZE,
                      0, 0, 0, 0));

    create_params[0] = metadata_start;
    create_params[1] = metadata_start + PAGE_DIRECTORY_SIZE;
    tvm_id = (uintptr_t)require_ok_silent(
        "CREATE_TVM",
        covh_call_to(tsm_domain_id, COVH_CREATE_TVM,
                      (uintptr_t)create_params, sizeof(create_params),
                      0, 0, 0, 0));
    if (tvm_id != 0)
        fail("expected isolated TSM-local TVM ID zero", (long)tvm_id);

    require_ok_silent(
        "ADD_MEMORY_REGION",
        covh_call_to(tsm_domain_id, COVH_ADD_MEMORY_REGION,
                      tvm_id, 0, GUEST_RAM_SIZE, 0, 0, 0));
    guest_entry = load_guest(tsm_domain_id, tvm_id, guest_start);
    require_ok_silent(
        "CREATE_VCPU",
        covh_call_to(tsm_domain_id, COVH_CREATE_VCPU,
                      tvm_id, 0, 0, 0, 0, 0));
    require_ok_silent(
        "FINALIZE_TVM",
        covh_call_to(tsm_domain_id, COVH_FINALIZE_TVM,
                      tvm_id, guest_entry, tsm_domain_id, 0, 0, 0));
    return tvm_id;
}

static void destroy_and_reclaim(uintptr_t tsm_domain_id, uintptr_t tvm_id,
                                uintptr_t metadata_start,
                                uintptr_t guest_start)
{
    require_ok_silent(
        "DESTROY_TVM",
        covh_call_to(tsm_domain_id, COVH_DESTROY_TVM,
                      tvm_id, 0, 0, 0, 0, 0));
    require_ok_silent(
        "RECLAIM_METADATA",
        covh_call_to(tsm_domain_id, COVH_RECLAIM_PAGES,
                      metadata_start, PER_TVM_METADATA_SIZE / PAGE_SIZE,
                      0, 0, 0, 0));
    require_ok_silent(
        "RECLAIM_GUEST",
        covh_call_to(tsm_domain_id, COVH_RECLAIM_PAGES,
                      guest_start, GUEST_RAM_SIZE / PAGE_SIZE,
                      0, 0, 0, 0));
}

int main(void)
{
    uintptr_t metadata1 = (uintptr_t)__confidential_metadata_start;
    uintptr_t metadata2 = metadata1 + PER_TVM_METADATA_SIZE;
    uintptr_t guest1 = (uintptr_t)__confidential_guest_start;
    uintptr_t guest2 = guest1 + GUEST_RAM_SIZE;
    uintptr_t tvm1;
    uintptr_t tvm2;
    struct sbiret ret;

    if (metadata2 + PER_TVM_METADATA_SIZE >
            (uintptr_t)__confidential_metadata_end ||
        guest2 + GUEST_RAM_SIZE > (uintptr_t)__confidential_guest_end)
        fail("launcher memory layout is too small", -1);

    ret = sbi_call(SBI_EXT_SUPD, SBI_SUPD_GET_ACTIVE,
                   0, 0, 0, 0, 0, 0);
    require_ok_silent("GET_ACTIVE_DOMAINS", ret);
    if (((uintptr_t)ret.value &
         (DOMAIN_BIT(0) | DOMAIN_BIT(PRIMARY_TSM_DOMAIN) |
          DOMAIN_BIT(UNTRUSTED_DOMAIN) | DOMAIN_BIT(SECONDARY_TSM_DOMAIN))) !=
        (DOMAIN_BIT(0) | DOMAIN_BIT(PRIMARY_TSM_DOMAIN) |
         DOMAIN_BIT(UNTRUSTED_DOMAIN) | DOMAIN_BIT(SECONDARY_TSM_DOMAIN)))
        fail("not all supervisor domains are active", -1);

    clear_bytes(__confidential_metadata_start,
                (size_t)(__confidential_metadata_end -
                         __confidential_metadata_start));
    clear_bytes(__confidential_guest_start,
                (size_t)(__confidential_guest_end -
                         __confidential_guest_start));

    puts("[HOST] creating TVM for trusted domain 1\n");
    tvm1 = create_tvm(PRIMARY_TSM_DOMAIN, metadata1, guest1);
    puts("[HOST] creating TVM for trusted domain 3\n");
    tvm2 = create_tvm(SECONDARY_TSM_DOMAIN, metadata2, guest2);

    puts("[HOST] running TVM for trusted domain 1\n");
    require_ok_silent(
        "RUN_TVM_1",
        covh_call_to(PRIMARY_TSM_DOMAIN, COVH_RUN_VCPU,
                      tvm1, 0, 0, 0, 0, 0));
    puts("[HOST] TVM 1 returned\n");

    puts("[HOST] running TVM for trusted domain 3\n");
    require_ok_silent(
        "RUN_TVM_2",
        covh_call_to(SECONDARY_TSM_DOMAIN, COVH_RUN_VCPU,
                      tvm2, 0, 0, 0, 0, 0));
    puts("[HOST] TVM 2 returned\n");

    destroy_and_reclaim(PRIMARY_TSM_DOMAIN, tvm1, metadata1, guest1);
    destroy_and_reclaim(SECONDARY_TSM_DOMAIN, tvm2, metadata2, guest2);

    puts("[HOST] PASS: multi-supervisor-domain TVMs completed\n");
    shutdown();
}
