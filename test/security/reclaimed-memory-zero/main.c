#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"

#define PAGE_SIZE               4096UL
#define PAGE_DIRECTORY_SIZE     (64UL *  PAGE_SIZE)
#define TVM_STATE_SIZE          PAGE_SIZE
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE          (64UL * 1024UL * 1024UL)
#endif
#define SEGMENT_STAGING_SIZE    (2UL  *  1024UL * 1024UL)

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern const unsigned char __guest_dtb[];
extern const unsigned char __guest_dtb_end[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static const uintptr_t metadata_start = (uintptr_t)__confidential_metadata_start;
static const uintptr_t metadata_end = (uintptr_t)__confidential_metadata_end;
static const uintptr_t guest_memory_start = (uintptr_t)__confidential_guest_start;
static const uintptr_t guest_memory_end = (uintptr_t)__confidential_guest_end;
static const uintptr_t page_table = metadata_start;
static const uintptr_t tvm_state = page_table + PAGE_DIRECTORY_SIZE;
static const uintptr_t guest_physical = guest_memory_start;
static const size_t guest_elf_size = (size_t) __guest_elf_size;

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));


static void assert_zeroed(const char *name, uintptr_t start, uintptr_t end)
{
    volatile const unsigned char *memory =
        (volatile const unsigned char *)start;

    for (uintptr_t address = start; address < end; ++address) {
        unsigned char value = memory[address - start];

        if (value != 0) {
            puts("[ATTACK] leaked byte in ");
            puts(name);
            puts(" at ");
            puthex(address);
            puts(": ");
            puthex(value);
            putchar('\n');
            puts("halting");
            halt();
        }
    }

    puts("[ATTACK] ");
    puts(name);
    puts(" contains only zeroes\n");
}

static void info_stealer()
{
    puts("[ATTACK] reading reclaimed TVM memory\n");
    assert_zeroed("metadata", metadata_start, metadata_end);
    assert_zeroed("guest RAM", guest_memory_start, guest_memory_end);
    puts("[ATTACK] PASS: no reclaimed TVM data is visible\n");
}

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
        .require_ok = require_ok,
        .fail = fail,
    };

    uintptr_t entry = baremetal_load_guest_elf(tvm_id, &loader);

    puts("[HOST] Guest entry: ");
    puthex(entry);
    puts("\n");
    return entry;
}

static void create_and_run_tvm() {

    struct sbiret ret;
    uintptr_t tvm_id;
    uintptr_t guest_entry;
    uintptr_t create_params[2] __attribute__((aligned(16)));

    puts("\n[HOST] Creating TVM\n");
    puts("[HOST] Embedded ELF: ");
    puthex((uintptr_t)__guest_elf);
    puts("-");
    puthex((uintptr_t)__guest_elf  + guest_elf_size - 1);
    if ((uintptr_t)__guest_dtb != (uintptr_t)__guest_dtb_end) {
        puts("\n[HOST] Embedded DTB: \n");
        puthex((uintptr_t)__guest_dtb);
        puts("-");
        puthex((uintptr_t)__guest_dtb_end - 1);
    }
    puts("\n[HOST] Confidential metadata: ");
    puthex(metadata_start);
    puts("-");
    puthex(metadata_end - 1);
    puts("\n[HOST] Confidential guest RAM: ");
    puthex(guest_memory_start);
    puts("-");
    puthex(guest_memory_end - 1);
    puts("\n");

    ret = sbi_call(SBI_EXT_SUPD, SBI_SUPD_GET_ACTIVE,
                   0, 0, 0, 0, 0, 0);
    require_ok("GET_ACTIVE_DOMAINS", ret);
    if (((uintptr_t)ret.value & 0x3) != 0x3)
        fail("TSM domain is not active", -1);

    /* ADD_ZERO_PAGES maps without clearing; initialize pages before donation. */
    clear_bytes(__confidential_metadata_start,
                (size_t)(__confidential_metadata_end -
                         __confidential_metadata_start));
    clear_bytes(__confidential_guest_start,
                (size_t)(__confidential_guest_end -
                         __confidential_guest_start));
    require_ok("CONVERT_META_PAGES",
               covh_call(COVH_CONVERT_PAGES,
                         metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE,
                         0, 0, 0, 0));
    require_ok("CONVERT_GUEST_PAGES",
               covh_call(COVH_CONVERT_PAGES,
                         guest_memory_start,
                         (guest_memory_end - guest_memory_start) / PAGE_SIZE,
                         0, 0, 0, 0));

    create_params[0] = page_table;
    create_params[1] = tvm_state;
    tvm_id = (uintptr_t)require_ok(
        "CREATE_TVM",
        covh_call(COVH_CREATE_TVM,
                  (uintptr_t)create_params, sizeof(create_params),
                  0, 0, 0, 0));

    require_ok("ADD_MEMORY_REGION",
               covh_call(COVH_ADD_MEMORY_REGION,
                         tvm_id, 0, GUEST_RAM_SIZE, 0, 0, 0));

    guest_entry = load_guest_elf(tvm_id, guest_physical);

    require_ok("CREATE_VCPU",
               covh_call(COVH_CREATE_VCPU, tvm_id, 0, 0, 0, 0, 0));
    require_ok("FINALIZE_TVM",
               covh_call(COVH_FINALIZE_TVM,
                         tvm_id, guest_entry, 0, 0, 0, 0));

    puts("[HOST] Entering TVM\n");
    ret = covh_call(COVH_RUN_VCPU, tvm_id, 0, 0, 0, 0, 0);

    puts("[HOST] TVM exited with code ");
    puthex(ret.error);
    putchar('\n');

    /* Destroy the TVM */
    require_ok("DESTROY_TVM",
               covh_call(COVH_DESTROY_TVM, tvm_id, 0, 0, 0,0,0));
    require_ok("RECLAIM_META_PAGES",
               covh_call(COVH_RECLAIM_PAGES,
                   metadata_start,
                   (metadata_end - metadata_start) / PAGE_SIZE,
                   0, 0,0,0));

    require_ok("RECLAIM_GUEST_PAGES",
               covh_call(COVH_RECLAIM_PAGES,
                   guest_memory_start,
                   (guest_memory_end - guest_memory_start) / PAGE_SIZE,
                   0, 0,0,0));

    puts("[HOST] TVM destroyed\n");
}

int main(void)
{
    puts("[HOST] Info stealing test after TVM disposal\n");
    create_and_run_tvm();
    /* Another process tries to access the same physical memory */
    info_stealer();
    puts("[HOST] Program completed. Shutting down\n");
    shutdown();
}
