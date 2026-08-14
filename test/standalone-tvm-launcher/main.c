#include <stddef.h>
#include <stdint.h>

#define PAGE_SIZE                  4096UL
#define PAGE_DIRECTORY_SIZE        (64UL * PAGE_SIZE)
#define TVM_STATE_SIZE             PAGE_SIZE
#define GUEST_RAM_SIZE             (64UL * 1024UL * 1024UL)
#define SEGMENT_STAGING_SIZE       (2UL * 1024UL * 1024UL)

#define SBI_EXT_DBCN               0x4442434EUL
#define SBI_DBCN_WRITE_BYTE        2UL
#define SBI_EXT_SUPD               0x53555044UL
#define SBI_SUPD_GET_ACTIVE        0UL
#define SBI_EXT_COVH               0x434F5648UL
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
#define COVH_REMOVE_PAGES          19UL

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
extern const unsigned char __guest_dtb[];
extern const unsigned char __guest_dtb_end[];
extern unsigned char __confidential_metadata_start[];
extern unsigned char __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[];
extern unsigned char __confidential_guest_end[];

static unsigned char segment_staging[SEGMENT_STAGING_SIZE]
    __attribute__((aligned(PAGE_SIZE)));

static struct sbiret sbi_call(uintptr_t eid, uintptr_t fid,
                              uintptr_t arg0, uintptr_t arg1,
                              uintptr_t arg2, uintptr_t arg3,
                              uintptr_t arg4, uintptr_t arg5) {
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
                               uintptr_t a4, uintptr_t a5) {
    return sbi_call(SBI_EXT_COVH, COVH_TARGET_TSM | fid,
                    a0, a1, a2, a3, a4, a5);
}

static void putchar(char c) {
    (void)sbi_call(SBI_EXT_DBCN, SBI_DBCN_WRITE_BYTE,
                   (uintptr_t)(unsigned char)c, 0, 0, 0, 0, 0);
}

static void puts(const char *s) {
    while (*s != '\0')
        putchar(*s++);
}

static void puthex(uintptr_t value) {
    static const char digits[] = "0123456789abcdef";

    puts("0x");
    for (int shift = (int)(sizeof(value) * 8) - 4; shift >= 0; shift -= 4)
        putchar(digits[(value >> shift) & 0xf]);
}

static void clear_bytes(void *address, size_t size) {
    volatile unsigned char *p = address;

    while (size-- != 0)
        *p++ = 0;
}

static void copy_bytes(void *destination, const void *source, size_t size) {
    unsigned char *dst = destination;
    const unsigned char *src = source;

    while (size-- != 0)
        *dst++ = *src++;
}

__attribute__((noreturn))
static void halt(void) {
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

    puts("[HOST] ");
    puts(operation);
    puts(" OK\n");
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
    size_t elf_size = (size_t)(__guest_elf_size);
    const Elf64_Ehdr *header;
    uintptr_t next_guest_page = 0;

    if (elf_size < sizeof(Elf64_Ehdr))
        fail("embedded ELF is too small", -1);

    header = (const Elf64_Ehdr *)elf;
    if (header->e_ident[0] != 0x7f || header->e_ident[1] != 'E' ||
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

    puts("[HOST] Guest entry: ");
    puthex((uintptr_t)header->e_entry);
    puts("\n");

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

        add_zero_pages(tvm_id, &next_physical, next_guest_page,
                       (guest_page - next_guest_page) / PAGE_SIZE);

        if (measured_pages != 0) {
            size_t measured_size = measured_pages * PAGE_SIZE;

            clear_bytes(segment_staging, measured_size);
            copy_bytes(segment_staging + page_offset,
                       elf + segment->p_offset,
                       (size_t)segment->p_filesz);
            check_guest_space(next_physical, measured_pages);
            require_ok("ADD_MEASURED_PAGES",
                       covh_call(COVH_ADD_MEASURED_PAGES,
                                 tvm_id, (uintptr_t)segment_staging,
                                 next_physical, 0, measured_pages,
                                 guest_page));
            next_physical += measured_size;
        }

        add_zero_pages(tvm_id, &next_physical,
                       guest_page + measured_pages * PAGE_SIZE,
                       total_pages - measured_pages);
        next_guest_page = segment_end;
    }

    if (next_guest_page > GUEST_RAM_SIZE)
        fail("guest ELF exceeds guest RAM", -1);

    add_zero_pages(tvm_id, &next_physical, next_guest_page,
                   (GUEST_RAM_SIZE - next_guest_page) / PAGE_SIZE);
    return (uintptr_t)header->e_entry;
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
    struct sbiret ret;
    uintptr_t tvm_id;
    uintptr_t guest_entry;
    size_t guest_elf_size = (size_t)__guest_elf_size;

    puts("\n[HOST] Standalone CoVE TVM launcher\n");
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

    puts("[HOST] Program completed. Halting\n");
    halt();
}
