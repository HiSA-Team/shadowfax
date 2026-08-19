#ifndef SHADOWFAX_BAREMETAL_H
#define SHADOWFAX_BAREMETAL_H

#include <stddef.h>
#include <stdint.h>

#define SBI_EXT_DBCN                    0x4442434eUL
#define SBI_DBCN_WRITE_BYTE             2UL
#define SBI_EXT_HSM                     0x0048534dUL
#define SBI_HSM_HART_SUSPEND            3UL
#define SBI_EXT_SRST                    0x53525354UL
#define SBI_SRST_RESET                  0UL
#define SBI_EXT_SUPD                    0x53555044UL
#define SBI_SUPD_GET_ACTIVE             0UL
#define SBI_EXT_COVH                    0x434f5648UL
#define COVH_TARGET_TSM                 (1UL << 26)
#define COVH_CONVERT_PAGES              1UL
#define COVH_RECLAIM_PAGES              2UL
#define COVH_CREATE_TVM                 5UL
#define COVH_FINALIZE_TVM               6UL
#define COVH_DESTROY_TVM                8UL
#define COVH_ADD_MEMORY_REGION          9UL
#define COVH_ADD_MEASURED_PAGES         11UL
#define COVH_ADD_ZERO_PAGES             12UL
#define COVH_CREATE_VCPU                14UL
#define COVH_RUN_VCPU                   15UL
#define COVH_REMOVE_PAGES               19UL

struct sbiret {
    long error;
    long value;
};

struct sbiret sbi_call(uintptr_t eid, uintptr_t fid,
                       uintptr_t arg0, uintptr_t arg1,
                       uintptr_t arg2, uintptr_t arg3,
                       uintptr_t arg4, uintptr_t arg5);
struct sbiret covh_call(uintptr_t fid,
                        uintptr_t arg0, uintptr_t arg1,
                        uintptr_t arg2, uintptr_t arg3,
                        uintptr_t arg4, uintptr_t arg5);

void putchar(char character);
void puts(const char *message);
void puthex(uintptr_t value);
void putdec(uint64_t value);
void clear_bytes(void *address, size_t size);
void copy_bytes(void *destination, const void *source, size_t size);
/* Alignment must be a non-zero power of two. */
uintptr_t align_down(uintptr_t value, size_t alignment);
uintptr_t align_up(uintptr_t value, size_t alignment);
__attribute__((noreturn)) void halt(void);
__attribute__((noreturn)) void shutdown(void);
__attribute__((noreturn)) void fail(const char *operation, long error);
long require_ok(const char *operation, struct sbiret result);
long require_ok_silent(const char *operation, struct sbiret result);

#endif
