#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"
#include "baremetal_elf.h"

#define PAGE_SIZE               4096UL
#define PAGE_DIRECTORY_SIZE     (64UL * PAGE_SIZE)
#define SEGMENT_STAGING_SIZE    (64UL * 1024UL)
#define SECRET_WORD             0xa5a5a5a5a5a5a5a5UL

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

/*
 * probe_load performs a single uncompressed load. probe_trap skips it on a
 * load access fault (scause 5) and records the fault in probe_faulted.
 */
volatile uintptr_t probe_faulted;
extern uintptr_t probe_load(uintptr_t address);
extern void probe_trap(void);

__asm__(
    "    .section .text\n"
    "    .align 2\n"
    "    .globl probe_load\n"
    "probe_load:\n"
    "    .option push\n"
    "    .option norvc\n"
    "    ld a0, 0(a0)\n"
    "    .option pop\n"
    "    ret\n"
    "    .align 2\n"
    "    .globl probe_trap\n"
    "probe_trap:\n"
    "    addi sp, sp, -16\n"
    "    sd t0, 0(sp)\n"
    "    sd t1, 8(sp)\n"
    "    csrr t0, scause\n"
    "    li t1, 5\n"
    "1:  bne t0, t1, 1b\n"
    "    la t0, probe_faulted\n"
    "    li t1, 1\n"
    "    sd t1, 0(t0)\n"
    "    csrr t0, sepc\n"
    "    addi t0, t0, 4\n"
    "    csrw sepc, t0\n"
    "    ld t0, 0(sp)\n"
    "    ld t1, 8(sp)\n"
    "    addi sp, sp, 16\n"
    "    sret\n");

static uintptr_t create_and_run_tvm(void)
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
    require_ok("CONVERT_META_PAGES",
        covh_call(COVH_CONVERT_PAGES, metadata,
                  ((uintptr_t)__confidential_metadata_end - metadata) / PAGE_SIZE,
                  0, 0, 0, 0));
    require_ok("CONVERT_GUEST_PAGES",
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

    puts("[HOST] Entering TVM\n");
    (void)covh_call(COVH_RUN_VCPU, tvm_id, 0, 0, 0, 0, 0);
    puts("[HOST] TVM returned\n");
    return tvm_id;
}

/* The host still owns the TVM, which is stopped but not destroyed. */
static void read_converted_memory(void)
{
    const uintptr_t start = (uintptr_t)__confidential_guest_start;
    const uintptr_t end = start + GUEST_RAM_SIZE;

    __asm__ volatile("csrw stvec, %0" :: "r"(probe_trap) : "memory");

    puts("[ATTACK] reading converted guest RAM\n");
    for (uintptr_t address = start; address < end; address += sizeof(uintptr_t)) {
        uintptr_t value;

        probe_faulted = 0;
        value = probe_load(address);
        if (probe_faulted) {
            puts("[ATTACK] load from ");
            puthex(address);
            puts(" raised an access fault\n");
            puts("[HOST] PASS: converted memory is not accessible\n");
            return;
        }
        if (value == SECRET_WORD) {
            puts("[ATTACK] FAIL: read TVM secret at ");
            puthex(address);
            putchar('\n');
            fail("TVM secret was readable", -1);
        }
    }
    fail("converted memory was readable", -1);
}

int main(void)
{
    puts("[HOST] Converted-memory isolation test\n");
    (void)create_and_run_tvm();
    read_converted_memory();
    shutdown();
}
