#ifndef SHADOWFAX_BAREMETAL_ELF_H
#define SHADOWFAX_BAREMETAL_ELF_H

#include <stdint.h>

#include "baremetal.h"

#define ELFCLASS64  2
#define ELFDATA2LSB 1
#define EM_RISCV    243
#define PT_LOAD     1U

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

struct baremetal_elf_loader {
    const unsigned char *elf;
    size_t elf_size;
    unsigned char *staging;
    size_t staging_size;
    uintptr_t physical_start;
    uintptr_t physical_end;
    size_t guest_ram_size;
    int premapped;
    long (*require_ok)(const char *operation, struct sbiret result);
    __attribute__((noreturn)) void (*fail)(const char *operation, long error);
};

uintptr_t baremetal_load_guest_elf(uintptr_t tvm_id,
                                   const struct baremetal_elf_loader *loader);
uintptr_t baremetal_load_guest_elf_to(uintptr_t tsm_domain_id,
                                      uintptr_t tvm_id,
                                      const struct baremetal_elf_loader *loader);

#endif
