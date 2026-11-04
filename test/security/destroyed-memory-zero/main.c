#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"

#define PAGE_SIZE               4096UL
#define PAGE_DIRECTORY_SIZE     (64UL *  PAGE_SIZE)
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE          (2UL * 1024UL * 1024UL)
#endif
#define SEGMENT_STAGING_SIZE    (64UL * 1024UL)

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static const uintptr_t metadata_start = (uintptr_t)__confidential_metadata_start;
static const uintptr_t metadata_end = (uintptr_t)__confidential_metadata_end;
static const uintptr_t guest_start = (uintptr_t)__confidential_guest_start;
static const uintptr_t guest_end = (uintptr_t)__confidential_guest_end;

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

/*
 * Build, run and destroy one TVM on the same confidential pages every time.
 * The pages stay converted between TVMs, so only DESTROY_TVM stands between
 * one TVM's data and the next TVM's zero pages.
 */
static void run_and_destroy_tvm(int round)
{
    uintptr_t create_params[2] __attribute__((aligned(16))) = {
        metadata_start, metadata_start + PAGE_DIRECTORY_SIZE,
    };
    const struct baremetal_elf_loader loader = {
        .elf = __guest_elf,
        .elf_size = (size_t)__guest_elf_size,
        .staging = segment_staging,
        .staging_size = sizeof(segment_staging),
        .physical_start = guest_start,
        .physical_end = guest_end,
        .guest_ram_size = GUEST_RAM_SIZE,
        .premapped = 0,
        .require_ok = require_ok,
        .fail = fail,
    };
    uintptr_t tvm_id, entry;

    puts("[HOST] TVM round ");
    putdec(round);
    putchar('\n');

    tvm_id = (uintptr_t)require_ok(
        "CREATE_TVM", covh_call(COVH_CREATE_TVM, (uintptr_t)create_params,
                                sizeof(create_params), 0, 0, 0, 0));
    require_ok("ADD_MEMORY_REGION",
               covh_call(COVH_ADD_MEMORY_REGION, tvm_id, 0, GUEST_RAM_SIZE, 0, 0, 0));
    entry = baremetal_load_guest_elf(tvm_id, &loader);
    require_ok("CREATE_VCPU", covh_call(COVH_CREATE_VCPU, tvm_id, 0, 0, 0, 0, 0));
    require_ok("FINALIZE_TVM", covh_call(COVH_FINALIZE_TVM, tvm_id, entry, 0, 0, 0, 0));
    (void)covh_call(COVH_RUN_VCPU, tvm_id, 0, 0, 0, 0, 0);
    require_ok("DESTROY_TVM", covh_call(COVH_DESTROY_TVM, tvm_id, 0, 0, 0, 0, 0));
}

int main(void)
{
    puts("[HOST] Zero pages after DESTROY_TVM without reclaim\n");

    require_ok("CONVERT_META_PAGES",
               covh_call(COVH_CONVERT_PAGES, metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE, 0, 0, 0, 0));
    require_ok("CONVERT_GUEST_PAGES",
               covh_call(COVH_CONVERT_PAGES, guest_start,
                         (guest_end - guest_start) / PAGE_SIZE, 0, 0, 0, 0));

    /* Round 1 plants a secret; round 2 fails if its zero pages still hold it. */
    run_and_destroy_tvm(1);
    run_and_destroy_tvm(2);

    require_ok("RECLAIM_META_PAGES",
               covh_call(COVH_RECLAIM_PAGES, metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE, 0, 0, 0, 0));
    require_ok("RECLAIM_GUEST_PAGES",
               covh_call(COVH_RECLAIM_PAGES, guest_start,
                         (guest_end - guest_start) / PAGE_SIZE, 0, 0, 0, 0));

    puts("[HOST] PASS: destroyed TVM data never reaches the next TVM\n");
    shutdown();
}
