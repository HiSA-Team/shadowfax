#include <stddef.h>
#include <stdint.h>

#define PAGE_SIZE 4096UL
#define PAGE_TABLE_SIZE (16UL * PAGE_SIZE)
#define GUEST_RAM_SIZE (16UL * 1024UL * 1024UL)
#define SECRET_GPA 0x300000UL
#define SBI_EXT_DBCN 0x4442434EUL
#define SBI_DBCN_WRITE_BYTE 2UL
#define SBI_EXT_SUPD 0x53555044UL
#define SBI_SUPD_GET_ACTIVE 0UL
#define SBI_EXT_COVH 0x434F5648UL
#define COVH_TARGET_TSM (1UL << 26)
#define COVH_CONVERT_PAGES 1UL
#define COVH_RECLAIM_PAGES 2UL
#define COVH_CREATE_TVM 5UL
#define COVH_FINALIZE_TVM 6UL
#define COVH_DESTROY_TVM 8UL
#define COVH_ADD_MEMORY_REGION 9UL
#define COVH_ADD_MEASURED_PAGES 11UL
#define COVH_ADD_ZERO_PAGES 12UL
#define COVH_CREATE_VCPU 14UL
#define COVH_RUN_TVM_VCPU 15UL
#define PT_LOAD 1U

struct sbiret {
    long error;
    long value;
};
typedef struct {
    unsigned char ident[16];
    uint16_t type, machine;
    uint32_t version;
    uint64_t entry, phoff, shoff;
    uint32_t flags;
    uint16_t ehsize, phentsize, phnum, shentsize, shnum, shstrndx;
} Elf64_Ehdr;
typedef struct {
    uint32_t type, flags;
    uint64_t offset, vaddr, paddr, filesz, memsz, align;
} Elf64_Phdr;

extern const unsigned char __guest_elf[];
extern const char __guest_elf_size[];
extern unsigned char __confidential_metadata_start[], __confidential_metadata_end[];
extern unsigned char __confidential_guest_start[], __confidential_guest_end[];
static unsigned char staging[2UL * 1024UL * 1024UL] __attribute__((aligned(PAGE_SIZE)));

static struct sbiret sbi_call(uintptr_t eid, uintptr_t fid, uintptr_t a0v, uintptr_t a1v,
                              uintptr_t a2v, uintptr_t a3v, uintptr_t a4v, uintptr_t a5v)
{
    register uintptr_t a0 asm("a0") = a0v, a1 asm("a1") = a1v, a2 asm("a2") = a2v,
                          a3 asm("a3") = a3v, a4 asm("a4") = a4v, a5 asm("a5") = a5v,
                          a6 asm("a6") = fid, a7 asm("a7") = eid;
    asm volatile("ecall"
                 : "+r"(a0), "+r"(a1)
                 : "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a6), "r"(a7)
                 : "memory");
    return (struct sbiret){(long)a0, (long)a1};
}

static struct sbiret covh_call(uintptr_t fid, uintptr_t a0, uintptr_t a1, uintptr_t a2,
                               uintptr_t a3, uintptr_t a4, uintptr_t a5)
{
    return sbi_call(SBI_EXT_COVH, COVH_TARGET_TSM | fid, a0, a1, a2, a3, a4, a5);
}

static void putchar(char c)
{
    (void)sbi_call(SBI_EXT_DBCN, SBI_DBCN_WRITE_BYTE, (unsigned char)c, 0, 0, 0, 0, 0);
}
static void puts(const char *s)
{
    while (*s)
        putchar(*s++);
}
static void puthex(uintptr_t value)
{
    static const char d[] = "0123456789abcdef";
    puts("0x");
    for (int i = (int)(sizeof(value) * 8) - 4; i >= 0; i -= 4)
        putchar(d[(value >> i) & 0xf]);
}
__attribute__((noreturn)) static void halt(void)
{
    for (;;)
        asm volatile("wfi");
}
__attribute__((noreturn)) static void fail(const char *op, long error)
{
    puts("[HOST] ERROR: ");
    puts(op);
    puts(" returned ");
    puthex((uintptr_t)error);
    puts("\n");
    halt();
}
static long ok(const char *op, struct sbiret ret)
{
    if (ret.error)
        fail(op, ret.error);
    return ret.value;
}
static uintptr_t down(uintptr_t x) { return x & ~(PAGE_SIZE - 1); }
static uintptr_t up(uintptr_t x) { return (x + PAGE_SIZE - 1) & ~(PAGE_SIZE - 1); }
static void clear(void *p, size_t n)
{
    volatile unsigned char *b = p;
    while (n--)
        *b++ = 0;
}
static void copy(void *d, const void *s, size_t n)
{
    unsigned char *dp = d;
    const unsigned char *sp = s;
    while (n--)
        *dp++ = *sp++;
}

static uintptr_t load_guest(uintptr_t tvmid, uintptr_t physical)
{
    const unsigned char *elf = __guest_elf;
    size_t size = (size_t)__guest_elf_size;
    const Elf64_Ehdr *eh = (const Elf64_Ehdr *)elf;
    uintptr_t next = 0;

    ok("ADD_ZERO_PAGES",
       covh_call(COVH_ADD_ZERO_PAGES, tvmid, physical, 0, GUEST_RAM_SIZE / PAGE_SIZE, 0, 0));
    if (size < sizeof(*eh) || eh->ident[0] != 0x7f || eh->ident[1] != 'E' || eh->ident[2] != 'L' ||
        eh->ident[3] != 'F' || eh->machine != 243)
        fail("invalid guest ELF", -1);
    for (uint16_t i = 0; i < eh->phnum; ++i) {
        const Elf64_Phdr *ph = (const Elf64_Phdr *)(elf + eh->phoff + (uint64_t)i * eh->phentsize);
        if (ph->type != PT_LOAD)
            continue;
        uintptr_t page = down((uintptr_t)ph->paddr);
        size_t pages = (size_t)up((uintptr_t)(ph->paddr - page) + ph->filesz) / PAGE_SIZE;
        if (ph->filesz > ph->memsz || ph->offset > size || ph->filesz > size - ph->offset ||
            page < next || pages * PAGE_SIZE > sizeof(staging))
            fail("invalid guest segment", -1);
        clear(staging, pages * PAGE_SIZE);
        copy(staging + ph->paddr - page, elf + ph->offset, ph->filesz);
        ok("ADD_MEASURED_PAGES", covh_call(COVH_ADD_MEASURED_PAGES, tvmid, (uintptr_t)staging,
                                           physical + page, 0, pages, page));
        next = page + (size_t)up((uintptr_t)(ph->paddr - page) + ph->memsz);
    }
    return (uintptr_t)eh->entry;
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
    clear((void *)meta, meta_end - meta);
    clear((void *)guest, guest_end - guest);
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
    halt();
}
