#include <stddef.h>
#include <stdint.h>

#define PAGE_SIZE                  4096UL
#define PAGE_DIRECTORY_SIZE        (16UL * PAGE_SIZE)
#define TVM_METADATA_SIZE          (16UL * PAGE_SIZE)
#define GUEST_RAM_SIZE             (16UL * 1024UL * 1024UL)
#define SEGMENT_STAGING_SIZE       (2UL * 1024UL * 1024UL)

#define SBI_EXT_DBCN               0x4442434eUL
#define SBI_DBCN_WRITE_BYTE        2UL
#define SBI_EXT_SUPD               0x53555044UL
#define SBI_SUPD_GET_ACTIVE        0UL
#define SBI_EXT_COVH               0x434f5648UL
#define COVH_TARGET_TSM            (1UL << 26)
#define COVH_CONVERT_PAGES         1UL
#define COVH_RECLAIM_PAGES         2UL
#define COVH_CREATE_TVM            5UL
#define COVH_FINALIZE_TVM          6UL
#define COVH_DESTROY_TVM           8UL
#define COVH_ADD_MEMORY_REGION     9UL
#define COVH_ADD_MEASURED_PAGES    11UL
#define COVH_ADD_ZERO_PAGES        12UL
#define COVH_CREATE_VCPU           14UL
#define COVH_RUN_VCPU              15UL

#define ELFCLASS64                 2
#define ELFDATA2LSB                1
#define EM_RISCV                   243
#define PT_LOAD                    1U

struct sbiret {
    long error;
    long value;
};

typedef struct {
    unsigned char e_ident[16];
    uint16_t e_type;
    uint16_t e_machine;
    uint32_t e_version;
    uint64_t e_entry;
    uint64_t e_phoff;
    uint64_t e_shoff;
    uint32_t e_flags;
    uint16_t e_ehsize;
    uint16_t e_phentsize;
    uint16_t e_phnum;
    uint16_t e_shentsize;
    uint16_t e_shnum;
    uint16_t e_shstrndx;
} Elf64_Ehdr;

typedef struct {
    uint32_t p_type;
    uint32_t p_flags;
    uint64_t p_offset;
    uint64_t p_vaddr;
    uint64_t p_paddr;
    uint64_t p_filesz;
    uint64_t p_memsz;
    uint64_t p_align;
} Elf64_Phdr;

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

static struct sbiret sbi_call(uintptr_t eid, uintptr_t fid,
                              uintptr_t arg0, uintptr_t arg1,
                              uintptr_t arg2, uintptr_t arg3,
                              uintptr_t arg4, uintptr_t arg5)
{
    register uintptr_t a0 asm("a0") = arg0;
    register uintptr_t a1 asm("a1") = arg1;
    register uintptr_t a2 asm("a2") = arg2;
    register uintptr_t a3 asm("a3") = arg3;
    register uintptr_t a4 asm("a4") = arg4;
    register uintptr_t a5 asm("a5") = arg5;
    register uintptr_t a6 asm("a6") = fid;
    register uintptr_t a7 asm("a7") = eid;

    asm volatile("ecall"
                 : "+r"(a0), "+r"(a1)
                 : "r"(a2), "r"(a3), "r"(a4), "r"(a5),
                   "r"(a6), "r"(a7)
                 : "memory");

    return (struct sbiret){(long)a0, (long)a1};
}

static struct sbiret covh_call(uintptr_t fid,
                               uintptr_t a0, uintptr_t a1,
                               uintptr_t a2, uintptr_t a3,
                               uintptr_t a4, uintptr_t a5)
{
    return sbi_call(SBI_EXT_COVH, COVH_TARGET_TSM | fid,
                    a0, a1, a2, a3, a4, a5);
}

static void putchar(char c)
{
    (void)sbi_call(SBI_EXT_DBCN, SBI_DBCN_WRITE_BYTE,
                   (uintptr_t)(unsigned char)c, 0, 0, 0, 0, 0);
}

static void puts(const char *message)
{
    while (*message != '\0')
        putchar(*message++);
}

static void puthex(uintptr_t value)
{
    static const char digits[] = "0123456789abcdef";

    puts("0x");
    for (int shift = (int)(sizeof(value) * 8) - 4; shift >= 0; shift -= 4)
        putchar(digits[(value >> shift) & 0xf]);
}

__attribute__((noreturn))
static void halt(void)
{
    for (;;)
        asm volatile("wfi");
}

__attribute__((noreturn))
static void fail(const char *operation, long error)
{
    puts("[HOST] ERROR: ");
    puts(operation);
    puts(" returned ");
    puthex((uintptr_t)error);
    puts("\n");
    halt();
}

static long require_ok(const char *operation, struct sbiret ret)
{
    if (ret.error != 0)
        fail(operation, ret.error);

    return ret.value;
}

static uintptr_t align_down(uintptr_t value)
{
    return value & ~(PAGE_SIZE - 1);
}

static uintptr_t align_up(uintptr_t value)
{
    return (value + PAGE_SIZE - 1) & ~(PAGE_SIZE - 1);
}

static void clear_bytes(void *address, size_t size)
{
    volatile unsigned char *p = address;

    while (size-- != 0)
        *p++ = 0;
}

static void copy_bytes(void *destination, const void *source, size_t size)
{
    unsigned char *dst = destination;
    const unsigned char *src = source;

    while (size-- != 0)
        *dst++ = *src++;
}

static void check_guest_space(uintptr_t next, size_t num_pages)
{
    uintptr_t end = next + num_pages * PAGE_SIZE;

    if (end < next || end > (uintptr_t)__confidential_guest_end)
        fail("confidential memory exhausted", -1);
}

static void add_zero_pages(uintptr_t tvm_id, uintptr_t *next_physical,
                           uintptr_t guest_page, size_t num_pages)
{
    if (num_pages == 0)
        return;

    check_guest_space(*next_physical, num_pages);
    require_ok("ADD_ZERO_PAGES",
               covh_call(COVH_ADD_ZERO_PAGES,
                         tvm_id, *next_physical, 0, num_pages,
                         guest_page, 0));
    *next_physical += num_pages * PAGE_SIZE;
}

static uintptr_t load_guest_elf(uintptr_t tvm_id, uintptr_t next_physical)
{
    const unsigned char *elf = __guest_elf;
    size_t elf_size = (size_t)__guest_elf_size;
    const Elf64_Ehdr *header = (const Elf64_Ehdr *)elf;
    uintptr_t next_guest_page = 0;
    uintptr_t physical_base = next_physical;

    add_zero_pages(tvm_id, &next_physical, 0,
                   GUEST_RAM_SIZE / PAGE_SIZE);

    if (elf_size < sizeof(*header) ||
        header->e_ident[0] != 0x7f || header->e_ident[1] != 'E' ||
        header->e_ident[2] != 'L' || header->e_ident[3] != 'F' ||
        header->e_ident[4] != ELFCLASS64 ||
        header->e_ident[5] != ELFDATA2LSB ||
        header->e_machine != EM_RISCV ||
        header->e_phentsize < sizeof(Elf64_Phdr))
        fail("invalid embedded RISC-V ELF", -1);

    if (header->e_phoff > elf_size ||
        header->e_phnum >
            (elf_size - (size_t)header->e_phoff) / header->e_phentsize)
        fail("invalid ELF program headers", -1);

    for (uint16_t i = 0; i < header->e_phnum; ++i) {
        const Elf64_Phdr *segment = (const Elf64_Phdr *)(
            elf + header->e_phoff + (uint64_t)i * header->e_phentsize);
        uintptr_t guest_page;
        uintptr_t page_offset;
        size_t measured_pages;
        size_t total_pages;
        uintptr_t segment_end;

        if (segment->p_type != PT_LOAD)
            continue;

        if (segment->p_filesz > segment->p_memsz ||
            segment->p_offset > elf_size ||
            segment->p_filesz > elf_size - segment->p_offset ||
            segment->p_paddr > GUEST_RAM_SIZE ||
            segment->p_memsz > GUEST_RAM_SIZE - segment->p_paddr)
            fail("invalid PT_LOAD range", -1);

        guest_page = align_down((uintptr_t)segment->p_paddr);
        page_offset = (uintptr_t)segment->p_paddr - guest_page;
        measured_pages = (size_t)align_up(page_offset + segment->p_filesz) /
                         PAGE_SIZE;
        total_pages = (size_t)align_up(page_offset + segment->p_memsz) /
                      PAGE_SIZE;
        segment_end = guest_page + total_pages * PAGE_SIZE;

        if (guest_page < next_guest_page)
            fail("overlapping PT_LOAD pages", -1);
        if (measured_pages * PAGE_SIZE > sizeof(segment_staging))
            fail("PT_LOAD exceeds staging buffer", -1);

        if (measured_pages != 0) {
            size_t measured_size = measured_pages * PAGE_SIZE;

            clear_bytes(segment_staging, measured_size);
            copy_bytes(segment_staging + page_offset,
                       elf + segment->p_offset,
                       (size_t)segment->p_filesz);
            check_guest_space(physical_base + guest_page, measured_pages);
            require_ok("ADD_MEASURED_PAGES",
                       covh_call(COVH_ADD_MEASURED_PAGES,
                                 tvm_id, (uintptr_t)segment_staging,
                                 physical_base + guest_page, 0, measured_pages,
                                 guest_page));
        }

        next_guest_page = segment_end;
    }

    if (next_guest_page > GUEST_RAM_SIZE)
        fail("guest ELF exceeds guest RAM", -1);

    return (uintptr_t)header->e_entry;
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
    require_ok("GET_ACTIVE_DOMAINS", ret);
    if (((uintptr_t)ret.value & 0x3) != 0x3)
        fail("TSM domain is not active", -1);

    clear_bytes((void *)metadata_start, metadata_end - metadata_start);
    clear_bytes((void *)guest_start, guest_end - guest_start);

    require_ok("CONVERT_META_1",
               covh_call(COVH_CONVERT_PAGES,
                         metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE,
                         0, 0, 0, 0));
    require_ok("CONVERT_GUEST",
               covh_call(COVH_CONVERT_PAGES,
                         guest1, (2 * GUEST_RAM_SIZE) / PAGE_SIZE,
                         0, 0, 0, 0));

    create_tvm(page_table1, state1, guest1, 1, &tvm1);
    create_tvm(page_table2, state2, guest2, 2, &tvm2);

    puts("[HOST] running TVM 1\n");
    require_ok("RUN_TVM_1",
               covh_call(COVH_RUN_VCPU, tvm1, 0, 0, 0, 0, 0));

    puts("[HOST] running TVM 2\n");
    require_ok("RUN_TVM_2",
               covh_call(COVH_RUN_VCPU, tvm2, 0, 0, 0, 0, 0));

    require_ok("DESTROY_TVM_1",
               covh_call(COVH_DESTROY_TVM, tvm1, 0, 0, 0, 0, 0));
    require_ok("DESTROY_TVM_2",
               covh_call(COVH_DESTROY_TVM, tvm2, 0, 0, 0, 0, 0));

    require_ok("RECLAIM_META",
               covh_call(COVH_RECLAIM_PAGES,
                         metadata_start,
                         (metadata_end - metadata_start) / PAGE_SIZE,
                         0, 0, 0, 0));
    require_ok("RECLAIM_GUEST",
               covh_call(COVH_RECLAIM_PAGES,
                         guest1, (2 * GUEST_RAM_SIZE) / PAGE_SIZE,
                         0, 0, 0, 0));

    puts("[HOST] PASS: two TVMs completed\n");
    halt();
}
