#include "baremetal_elf.h"

#define PAGE_SIZE 4096UL

static void check_space(const struct baremetal_elf_loader *loader,
                        uintptr_t address, size_t pages)
{
    uintptr_t end = address + pages * PAGE_SIZE;

    if (end < address || end > loader->physical_end)
        loader->fail("confidential memory exhausted", -1);
}

static void add_zero_pages(uintptr_t tvm_id,
                           const struct baremetal_elf_loader *loader,
                           uintptr_t *next_physical, uintptr_t guest_page,
                           size_t pages)
{
    if (pages == 0)
        return;

    check_space(loader, *next_physical, pages);
    loader->require_ok("ADD_ZERO_PAGES",
                       covh_call(COVH_ADD_ZERO_PAGES,
                                 tvm_id, *next_physical, 0, pages,
                                 guest_page, 0));
    *next_physical += pages * PAGE_SIZE;
}

uintptr_t baremetal_load_guest_elf(uintptr_t tvm_id,
                                   const struct baremetal_elf_loader *loader)
{
    const Elf64_Ehdr *header;
    uintptr_t next_physical = loader->physical_start;
    uintptr_t next_guest_page = 0;

    if (loader->elf_size < sizeof(Elf64_Ehdr))
        loader->fail("embedded ELF is too small", -1);

    header = (const Elf64_Ehdr *)loader->elf;
    if (header->e_ident[0] != 0x7f || header->e_ident[1] != 'E' ||
        header->e_ident[2] != 'L' || header->e_ident[3] != 'F' ||
        header->e_ident[4] != ELFCLASS64 ||
        header->e_ident[5] != ELFDATA2LSB ||
        header->e_machine != EM_RISCV)
        loader->fail("embedded ELF header is unsupported", -1);

    if (header->e_phentsize < sizeof(Elf64_Phdr) ||
        header->e_phoff > loader->elf_size ||
        (uint64_t)header->e_phnum >
            (loader->elf_size - header->e_phoff) / header->e_phentsize)
        loader->fail("embedded ELF program headers are invalid", -1);

    if (loader->premapped)
        add_zero_pages(tvm_id, loader, &next_physical, 0,
                       loader->guest_ram_size / PAGE_SIZE);

    for (uint16_t index = 0; index < header->e_phnum; ++index) {
        const Elf64_Phdr *segment = (const Elf64_Phdr *)(
            loader->elf + header->e_phoff +
            (uint64_t)index * header->e_phentsize);
        uintptr_t guest_page;
        uintptr_t page_offset;
        uintptr_t segment_end;
        size_t measured_pages;
        size_t total_pages;
        uintptr_t measured_physical;

        if (segment->p_type != PT_LOAD)
            continue;
        if (segment->p_filesz > segment->p_memsz ||
            segment->p_offset > loader->elf_size ||
            segment->p_filesz > loader->elf_size - segment->p_offset)
            loader->fail("embedded ELF segment is invalid", -1);

        guest_page = align_down((uintptr_t)segment->p_paddr, PAGE_SIZE);
        page_offset = (uintptr_t)segment->p_paddr - guest_page;
        measured_pages = (size_t)align_up(page_offset + segment->p_filesz,
                                          PAGE_SIZE) / PAGE_SIZE;
        total_pages = (size_t)align_up(page_offset + segment->p_memsz,
                                       PAGE_SIZE) / PAGE_SIZE;
        segment_end = guest_page + total_pages * PAGE_SIZE;

        if (guest_page < next_guest_page ||
            segment_end < guest_page ||
            segment_end > loader->guest_ram_size ||
            measured_pages * PAGE_SIZE > loader->staging_size)
            loader->fail("embedded ELF segments are invalid", -1);

        if (!loader->premapped)
            add_zero_pages(tvm_id, loader, &next_physical, next_guest_page,
                           (guest_page - next_guest_page) / PAGE_SIZE);

        if (measured_pages != 0) {
            size_t measured_size = measured_pages * PAGE_SIZE;

            clear_bytes(loader->staging, measured_size);
            copy_bytes(loader->staging + page_offset,
                       loader->elf + segment->p_offset,
                       (size_t)segment->p_filesz);
            measured_physical = loader->premapped
                ? loader->physical_start + guest_page
                : next_physical;
            check_space(loader, measured_physical, measured_pages);
            loader->require_ok("ADD_MEASURED_PAGES",
                               covh_call(COVH_ADD_MEASURED_PAGES,
                                         tvm_id,
                                         (uintptr_t)loader->staging,
                                         measured_physical, 0,
                                         measured_pages, guest_page));
            if (!loader->premapped)
                next_physical += measured_size;
        }

        if (!loader->premapped)
            add_zero_pages(tvm_id, loader, &next_physical,
                           guest_page + measured_pages * PAGE_SIZE,
                           total_pages - measured_pages);
        next_guest_page = segment_end;
    }

    if (!loader->premapped)
        add_zero_pages(tvm_id, loader, &next_physical, next_guest_page,
                       (loader->guest_ram_size - next_guest_page) / PAGE_SIZE);

    return (uintptr_t)header->e_entry;
}
