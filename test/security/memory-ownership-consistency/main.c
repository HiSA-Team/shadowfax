#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"

#define PAGE_SIZE              4096UL
#define PAGE_DIRECTORY_SIZE    (64UL * PAGE_SIZE)

extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];

static const uintptr_t metadata_start =
    (uintptr_t)__confidential_metadata_start;
static const uintptr_t metadata_end =
    (uintptr_t)__confidential_metadata_end;
static const uintptr_t page_table =
    (uintptr_t)__confidential_metadata_start;
static const uintptr_t tvm_state =
    (uintptr_t)__confidential_metadata_start + PAGE_DIRECTORY_SIZE;

static void require_failure(const char *operation, struct sbiret ret)
{
    if (ret.error == 0)
        fail(operation, ret.error);
}

static uintptr_t create_tvm_with_owned_metadata(void)
{
    struct sbiret ret;
    uintptr_t create_params[2] __attribute__((aligned(16)));

    ret = sbi_call(SBI_EXT_SUPD, SBI_SUPD_GET_ACTIVE,
                   0, 0, 0, 0, 0, 0);
    require_ok("GET_ACTIVE_DOMAINS", ret);
    if (((uintptr_t)ret.value & 0x3) != 0x3)
        fail("TSM domain is not active", -1);

    clear_bytes(__confidential_metadata_start,
                (size_t)(__confidential_metadata_end -
                         __confidential_metadata_start));
    require_ok("CONVERT_META_PAGES",
               covh_call(COVH_CONVERT_PAGES,
                         metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE,
                         0, 0, 0, 0));

    create_params[0] = page_table;
    create_params[1] = tvm_state;
    return (uintptr_t)require_ok(
        "CREATE_TVM",
        covh_call(COVH_CREATE_TVM,
                  (uintptr_t)create_params, sizeof(create_params),
                  0, 0, 0, 0));
}

static void rejected_reclaim_preserves_ownership(uintptr_t tvm_id)
{
    const uintptr_t metadata_pages =
        (metadata_end - metadata_start) / PAGE_SIZE;
    struct sbiret ret;

    /*
     * Shadowfax still records the original conversion as one allocation, so
     * this exact-range reclaim passes its initial ownership validation.  The
     * TSM has split that range while assigning the page table and TVM state to
     * the TVM, and consequently rejects the reclaim.
     */
    ret = covh_call(COVH_RECLAIM_PAGES,
                    metadata_start, metadata_pages, 0, 0, 0, 0);
    require_failure("TSM accepted an owned metadata reclaim", ret);
    puts("[HOST] TSM rejected reclaim of TVM-owned metadata\n");

    /* The TVM remains usable after the rejected ownership transaction. */
    require_ok("ADD_MEMORY_REGION_AFTER_FAILURE",
               covh_call(COVH_ADD_MEMORY_REGION,
                         tvm_id, 0, PAGE_SIZE, 0, 0, 0));
}

int main(void)
{
    uintptr_t tvm_id;

    puts("[HOST] Test memory ownership consistency\n");

    tvm_id = create_tvm_with_owned_metadata();
    puts("[HOST] TVM ID: ");
    puthex(tvm_id);
    putchar('\n');

    rejected_reclaim_preserves_ownership(tvm_id);

    puts("[HOST] PASS: rejected reclaim left the TVM usable\n");
    shutdown();
}
