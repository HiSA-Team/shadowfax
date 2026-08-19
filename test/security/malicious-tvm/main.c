#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"

#define PAGE_SIZE               4096UL
#define PAGE_TABLE_SIZE         (16UL *  PAGE_SIZE)
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE          (16UL *  1024UL * 1024UL)
#endif
#define SECRET_GPA              0x300000UL
#define COVH_RUN_TVM_VCPU COVH_RUN_VCPU
#define ok require_ok_silent

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[], __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[], __confidential_guest_end[];
static unsigned char staging[2UL * 1024UL * 1024UL] __attribute__((aligned(PAGE_SIZE)));

static uintptr_t load_guest(uintptr_t tvm_id, uintptr_t physical)
{
    const struct baremetal_elf_loader loader = {
        .elf = __guest_elf,
        .elf_size = (size_t)__guest_elf_size,
        .staging = staging,
        .staging_size = sizeof(staging),
        .physical_start = physical,
        .physical_end = (uintptr_t)__confidential_guest_end,
        .guest_ram_size = GUEST_RAM_SIZE,
        .premapped = 1,
        .require_ok = ok,
        .fail = fail,
    };

    return baremetal_load_guest_elf(tvm_id, &loader);
}

static uintptr_t create_tvm(uintptr_t pt, uintptr_t state, uintptr_t physical, uintptr_t role)
{
    uintptr_t params[2] __attribute__((aligned(16))) = {pt, state};
    uintptr_t id = (uintptr_t)ok(
        "CREATE_TVM", covh_call(COVH_CREATE_TVM, (uintptr_t)params, sizeof(params), 0, 0, 0, 0));
    ok("ADD_MEMORY_REGION", covh_call(COVH_ADD_MEMORY_REGION, id, 0, GUEST_RAM_SIZE, 0, 0, 0));
    uintptr_t entry = load_guest(id, physical);
    ok("CREATE_VCPU", covh_call(COVH_CREATE_VCPU, id, 0, 0, 0, 0, 0));
    ok("FINALIZE_TVM", covh_call(COVH_FINALIZE_TVM, id, entry, role, 0, 0, 0));
    return id;
}

int main(void)
{
    uintptr_t meta = (uintptr_t)__confidential_metadata_start;
    uintptr_t meta_end = (uintptr_t)__confidential_metadata_end;
    uintptr_t guest = (uintptr_t)__confidential_guest_start;
    uintptr_t guest_end = (uintptr_t)__confidential_guest_end;
    uintptr_t pt1 = meta, state1 = pt1 + PAGE_TABLE_SIZE;
    uintptr_t pt2 = meta + 32UL * PAGE_SIZE, state2 = pt2 + PAGE_TABLE_SIZE;
    uintptr_t guest1 = guest, guest2 = guest + GUEST_RAM_SIZE;
    if (guest2 + GUEST_RAM_SIZE > guest_end || state2 + PAGE_SIZE > meta_end)
        fail("layout", -1);
    struct sbiret ret = sbi_call(SBI_EXT_SUPD, SBI_SUPD_GET_ACTIVE, 0, 0, 0, 0, 0, 0);
    ok("GET_ACTIVE", ret);
    clear_bytes((void *)meta, meta_end - meta);
    clear_bytes((void *)guest, guest_end - guest);
    ok("CONVERT_META_PAGES",
       covh_call(COVH_CONVERT_PAGES, meta, (meta_end - meta) / PAGE_SIZE, 0, 0, 0, 0));
    ok("CONVERT_GUEST_PAGES",
       covh_call(COVH_CONVERT_PAGES, guest1, (2 * GUEST_RAM_SIZE) / PAGE_SIZE, 0, 0, 0, 0));
    uintptr_t trusted = create_tvm(pt1, state1, guest1, 1);
    uintptr_t malicious = create_tvm(pt2, state2, guest2, 2);

    puts("[HOST] running trusted TVM\n");
    ok("RUN_TRUSTED_TVM", covh_call(COVH_RUN_TVM_VCPU, trusted, 0, 0, 0, 0, 0));

    puts("[ATTACK] attempting physical alias\n");
    ret = covh_call(COVH_ADD_ZERO_PAGES, malicious, guest1 + SECRET_GPA, 0, 1, SECRET_GPA, 0);
    if (ret.error == 0)
        fail("physical alias accepted", ret.error);
    puts("[ATTACK] physical alias rejected\n");

    puts("[HOST] running malicious TVM\n");
    ok("RUN_MALICIOUS_TVM", covh_call(COVH_RUN_TVM_VCPU, malicious, 0, 0, 0, 0, 0));
    puts("[HOST] resuming trusted TVM\n");
    ok("RUN_TRUSTED_TVM_RESUME", covh_call(COVH_RUN_TVM_VCPU, trusted, 0, 0, 0, 0, 0));
    ok("DESTROY_TRUSTED_TVM", covh_call(COVH_DESTROY_TVM, trusted, 0, 0, 0, 0, 0));
    ok("DESTROY_MALICIOUS_TVM", covh_call(COVH_DESTROY_TVM, malicious, 0, 0, 0, 0, 0));
    ok("RECLAIM_METADATA_PAGES",
       covh_call(COVH_RECLAIM_PAGES, meta, (meta_end - meta) / PAGE_SIZE, 0, 0, 0, 0));
    ok("RECLAIM_GUEST_PAGES",
       covh_call(COVH_RECLAIM_PAGES, guest1, (2 * GUEST_RAM_SIZE) / PAGE_SIZE, 0, 0, 0, 0));
    puts("[HOST] PASS: malicious TVM could not access trusted data\n");
    shutdown();
}
