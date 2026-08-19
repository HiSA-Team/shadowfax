#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"

#define PAGE_SIZE              4096UL
#define PAGE_DIRECTORY_SIZE    (160UL                *  PAGE_SIZE)
#define TVM_STATE_SIZE         PAGE_SIZE
#define METADATA_SIZE          (1UL                  *  1024UL * 1024UL)
#ifndef GUEST_RAM_SIZE
#define GUEST_RAM_SIZE         (256UL                *  1024UL * 1024UL)
#endif
#define PAYLOAD_SIZE           (32UL                 *  1024UL * 1024UL)
#define SEGMENT_STAGING_SIZE   (2UL                  *  1024UL * 1024UL)
#define MEASURED_CHUNK_PAGES   (SEGMENT_STAGING_SIZE /  PAGE_SIZE)
#define MEASURED_CHUNKS        (PAYLOAD_SIZE         /  SEGMENT_STAGING_SIZE)
#define COVH_CALL_COUNT        (MEASURED_CHUNKS + 7)

#define COVH_ADD_MEASURED      11UL
#define COVH_ADD_ZERO          12UL

struct counters {
    uint64_t cycle;
    uint64_t instret;
    uint64_t time;
};

struct latency {
    const char *operation;
    struct counters before;
    struct counters after;
};

extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

static uint64_t read_cycle(void)
{
    uint64_t value;
    asm volatile("rdcycle %0" : "=r"(value));
    return value;
}

static uint64_t read_instret(void)
{
    uint64_t value;
    asm volatile("rdinstret %0" : "=r"(value));
    return value;
}

static uint64_t read_time(void)
{
    uint64_t value;
    asm volatile("rdtime %0" : "=r"(value));
    return value;
}

static struct counters read_counters(void)
{
    return (struct counters){read_cycle(), read_instret(), read_time()};
}

static void emit_latency(const char *kind, const char *operation,
                         struct counters before, struct counters after)
{
    puts("LATENCY,");
    puts(kind);
    putchar(',');
    puts(operation);
    putchar(',');
    putdec(after.cycle - before.cycle);
    putchar(',');
    putdec(after.instret - before.instret);
    putchar(',');
    putdec(after.time - before.time);
    putchar('\n');
}

static struct sbiret measured_covh(struct latency *latency,
                                   const char *operation, uintptr_t fid,
                                   uintptr_t a0, uintptr_t a1,
                                   uintptr_t a2, uintptr_t a3,
                                   uintptr_t a4, uintptr_t a5)
{
    latency->operation = operation;
    puts("[HOST] Calling ");
    puts(operation);
    puts("\n");
    latency->before = read_counters();
    struct sbiret ret = covh_call(fid, a0, a1, a2, a3, a4, a5);
    latency->after = read_counters();

    if (ret.error != 0)
        fail(operation, ret.error);
    return ret;
}

static void initialize_segment_staging(void)
{
    for (size_t i = 0; i < sizeof(segment_staging); ++i)
        segment_staging[i] = 0xa5;
}

int main(void)
{
    uintptr_t metadata_start = (uintptr_t)__confidential_metadata_start;
    uintptr_t metadata_end = (uintptr_t)__confidential_metadata_end;
    uintptr_t guest_start = (uintptr_t)__confidential_guest_start;
    uintptr_t guest_end = (uintptr_t)__confidential_guest_end;
    uintptr_t page_table = metadata_start;
    uintptr_t tvm_state = page_table + PAGE_DIRECTORY_SIZE;
    uintptr_t tvm_id;
    uintptr_t create_params[2] __attribute__((aligned(16)));
    struct latency latencies[COVH_CALL_COUNT];
    size_t latency_count = 0;
    struct sbiret ret;
    struct counters startup = {0};

    if (guest_end - guest_start != GUEST_RAM_SIZE || tvm_state + TVM_STATE_SIZE > metadata_end)
        fail("invalid launcher memory layout", -1);

    initialize_segment_staging();
    ret = sbi_call(SBI_EXT_SUPD, SBI_SUPD_GET_ACTIVE,
                   0, 0, 0, 0, 0, 0);
    if (ret.error != 0 || ((uintptr_t)ret.value & 0x3) != 0x3)
        fail("GET_ACTIVE_DOMAINS", ret.error != 0 ? ret.error : -1);

    measured_covh(&latencies[latency_count++], "CONVERT_META_PAGES", COVH_CONVERT_PAGES,
                  metadata_start,
                  (metadata_end - metadata_start) / PAGE_SIZE,
                  0, 0, 0, 0);
    measured_covh(&latencies[latency_count++], "CONVERT_GUEST_PAGES", COVH_CONVERT_PAGES,
                  guest_start, (guest_end - guest_start) / PAGE_SIZE,
                  0, 0, 0, 0);

    create_params[0] = page_table;
    create_params[1] = tvm_state;
    tvm_id = (uintptr_t)measured_covh(&latencies[latency_count++], "CREATE_TVM", COVH_CREATE_TVM,
                                      (uintptr_t)create_params,
                                      sizeof(create_params), 0, 0, 0, 0).value;
    measured_covh(&latencies[latency_count++], "ADD_MEMORY_REGION", COVH_ADD_MEMORY_REGION,
                  tvm_id, 0, GUEST_RAM_SIZE, 0, 0, 0);
    for (size_t chunk = 0; chunk < MEASURED_CHUNKS; ++chunk) {
        uintptr_t guest_offset = chunk * SEGMENT_STAGING_SIZE;

        measured_covh(&latencies[latency_count++], "ADD_MEASURED_PAGES", COVH_ADD_MEASURED,
                      tvm_id, (uintptr_t)segment_staging,
                      guest_start + guest_offset, 0,
                      MEASURED_CHUNK_PAGES, guest_offset);
    }
    measured_covh(&latencies[latency_count++], "ADD_ZERO_PAGES", COVH_ADD_ZERO,
                  tvm_id, guest_start + PAYLOAD_SIZE, 0,
                  (GUEST_RAM_SIZE - PAYLOAD_SIZE) / PAGE_SIZE,
                  PAYLOAD_SIZE, 0);
    measured_covh(&latencies[latency_count++], "CREATE_VCPU", COVH_CREATE_VCPU,
                  tvm_id, 0, 0, 0, 0, 0);
    measured_covh(&latencies[latency_count++], "FINALIZE_TVM", COVH_FINALIZE_TVM,
                  tvm_id, 0, 0, 0, 0, 0);

    for (size_t i = 0; i < latency_count; ++i) {
        startup.cycle += latencies[i].after.cycle - latencies[i].before.cycle;
        startup.instret += latencies[i].after.instret - latencies[i].before.instret;
        startup.time += latencies[i].after.time - latencies[i].before.time;
        emit_latency("covh", latencies[i].operation,
                     latencies[i].before, latencies[i].after);
    }
    emit_latency("tvm_startup", "TVM_STARTUP", (struct counters){0}, startup);

    puts("[HOST] PASS: TVM launch latency measured\n");
    (void)sbi_call(SBI_EXT_SRST, 0, 0, 0, 0, 0, 0, 0);
    halt();
}
