# This program simulates a Trusted Virtual Machine (TVM) using an ELF image.
# Memory is divided between an Untrusted OS (emulated by this script) and the Trusted OS.
#
# Memory Layout:
# Untrusted OS:
#   - Total memory: 4 KB for code, 4 KB scratch space, 4KB for the program.
#   - Code section (4 KB) at UNTRUSTED_RAM_BASE contains ECALL and NOP instructions.
#   - Scratch memory (4 KB) for temporary computations which is at UNTRUSTED_RAM_BASE + 0x1000
#   - Program (ELF) is loaded at UNTRUSTED_RAM_BASE + 0x2000.
#
# Trusted OS (TVM):
#   - Receives 1024 pages (4 KB each) donated by the Untrusted OS, totaling 4 MB.
#   - First 16 KB of Trusted memory reserved for page tables.
#   - 4 KB reserved for the guest image.
#   - Trusted memory starts at UNTRUSTED_RAM_BASE + 0x4000.
#
# Author: Giuseppe Capasso <capassog97@gmail.com>
import os
import struct
import sys
import gdb
from typing import Dict, Optional
from elftools.elf.elffile import ELFFile

dirpath = os.path.join(os.getcwd(), "test")
if dirpath not in sys.path:
    sys.path.insert(0, dirpath)

from riscv_tee import Step, Runner, Domain, read_mem

# ================ CoVE Constants ========================#
EID_SUPD_ID: int = 0x53555044
EID_COVH_ID: int = 0x434F5648

SUPD_GET_ACTIVE_DOMAINS: int = 0
COVH_GET_TSM_INFO: int = 0
COVH_CONVERT_PAGES: int = 1
COVH_CREATE_TVM: int = 5
COVH_FINALIZE_TVM: int = 6
COVH_DESTROY_TVM: int = 8
COVH_ADD_MEMORY_REGION: int = 9
COVH_ADD_TVM_MEASURED_PAGES: int = 11
COVH_ADD_TVM_ZERO_PAGES: int = 12
COVH_CREATE_TVM_VCPU: int = 14
COVH_RUN_TVM_VCPU: int = 15

# ================ Create TVM Input ======================= #
TVM_ELF_PATH: str = "guests/coremark/coremark.bin"
TVM_BIN_PATH: str = "a.bin"
DEFAULT_PAGE_SIZE: int = 4096 # 4k
PAGE_DIRECTORY_SIZE: int = 0x4000  # 16kib
NUM_PAGES_TO_DONATE: int = 1024
TVM_RAM_SIZE: int = 1024 * 128 # 16k Guest RAM
TVM_VCPU_STATE_SIZE: int = 0x1000 #4k

TVM_ID: int = 1
VCPU_ID: int = 0
PAGE_SIZE: int = 0x1000  # 4k
PAGE_SIZE_TO_ID = {0x1000: 0}


# Asserts to check memory size
assert os.path.getsize(TVM_BIN_PATH) < TVM_RAM_SIZE, "insufficient Guest RAM (make sure guest ram < 1Mb)"

def ecall_ok(prev: Optional[Dict], curr: Dict) -> None:
    regs = curr["regs"]
    a0 = regs["a0"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"

def assert_get_active_domains(prev: Optional[Dict], curr: Dict) -> None:
    regs = curr["regs"]
    a0 = regs["a0"]
    a1 = regs["a1"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"
    assert a1 & 0x3 == 3, (
        f"a1 must be contains tsm (id=1) and the root domain (id=0) bit set (0x3) (current {a1})"
    )


def assert_get_tsm_info(prev: Optional[Dict], curr: Dict) -> None:
    # Example: ensure the ECALL returned successfully in a0 and left a1 = 48
    regs = curr["regs"]
    a0 = regs["a0"]
    a1 = regs["a1"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"
    assert a1 == 48, f"a1 must contain size 48 (current {a1})"

    assert prev is not None, "expecting the previous context not None"
    tsm_info_addr = prev["regs"]["a0"]

    # read TsmInfo as bytes in one shot. The TsmInfo struct is defined in "common/src/lib.rs" as follows:
    # Due to the memory alignement (TsmState is a u32), there is an extra u32 before the capabilities
    # struct TsmInfo {
    #     pub tsm_state: TsmState,
    #     pub tsm_impl_id: u32,
    #     pub tsm_version: u32,
    #     pub tsm_capabilities: usize,
    #     pub tvm_state_pages: usize,
    #     pub tvm_max_vcpus: usize,
    #     pub tvm_vcpu_state_pages: usize,
    # }
    tsm_info_bytes = (4 * 4) + (8 * 4)
    raw = read_mem(tsm_info_addr, tsm_info_bytes)
    if not isinstance(raw, (bytes, bytearray)) or len(raw) < tsm_info_bytes:
        raise AssertionError(
            f"failed to read {tsm_info_bytes} bytes at {hex(tsm_info_addr)}: {raw!r}"
        )

    (
        tsm_state,
        tsm_impl_id,
        tsm_version,
        _padding,
        tsm_capabilities,
        tvm_state_pages,
        tvm_max_vcpus,
        tvm_vcpu_state_pages,
    ) = struct.unpack("<IIIIQQQQ", raw)

    assert tsm_state == 2, f"tsm_state must be 2; current {tsm_state}"
    assert tsm_impl_id == 69, f"tsm_impl_id must be 69; current {tsm_impl_id}"
    assert tsm_version == 69, f"tsm_version must be 69; current  {tsm_version}"
    assert tsm_capabilities == 0, (
        f"tsm_capabilities must be 0; current {tsm_capabilities}"
    )
    assert tvm_state_pages == 1, f"tvm_state_pages must be 0; current {tvm_state_pages}"
    assert tvm_max_vcpus == 1, f"tvm_max_vcpus must be 1; current {tvm_max_vcpus}"
    assert tvm_vcpu_state_pages == 1, (
        f"tvm_vcpu_state_pages must be 1; current {tvm_vcpu_state_pages}"
    )

def setup_create_tvm(*args) -> None:
    # ecall parameter where to store the address
    tvm_params_addr = args[0]
    confidential_memory_start_addr = args[1]

    tvm_directory_addr = confidential_memory_start_addr
    tvm_state_addr = confidential_memory_start_addr + PAGE_DIRECTORY_SIZE

    # page table address in confidential memory
    tvm_directory_addr = struct.pack("<Q", tvm_directory_addr)
    tvm_state_addr = struct.pack("<Q", tvm_state_addr)

    inf = gdb.selected_inferior()

    inf.write_memory(tvm_params_addr, tvm_directory_addr)
    inf.write_memory(tvm_params_addr + 8, tvm_state_addr)



def assert_create_tvm(prev: Optional[Dict], curr: Dict) -> None:
    regs = curr["regs"]
    a0 = regs["a0"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"

    # assert that the pagetable is zero
    params_addr = prev["regs"]["a0"]

    # read two 64-bit words (page_table_addr, state_addr)
    raw = read_mem(params_addr, 16)
    if not isinstance(raw, (bytes, bytearray)) or len(raw) < 16:
        raise AssertionError(f"failed to read 16 bytes at {hex(params_addr)}: {raw!r}")

    page_table_addr, state_addr = struct.unpack("<QQ", raw)

    raw = read_mem(page_table_addr, PAGE_DIRECTORY_SIZE)
    if not isinstance(raw, (bytes, bytearray)):
        raise AssertionError(
            f"failed to read {PAGE_DIRECTORY_SIZE} bytes at {hex(page_table_addr)}: {raw!r}"
        )
    assert raw == b"\x00" * PAGE_DIRECTORY_SIZE, (
        "page table must be zero after TVM creation"
    )

def align_up(addr: int, align: int) -> int:
    return (addr + align - 1) & ~(align - 1)


def align_down(addr: int, align: int) -> int:
    return (addr) & ~(align - 1)


def load_guest_elf_and_make_steps(
        path: str, untrusted_base_addr: int, trusted_physical_addr: int
) -> (int, list, int, int):
    """
    Load ELF segments into memory via GDB and creates step to map into confidential domain

    Args:
        path: Path to ELF file
        untrusted_base_addr: Base address where to load the tvm
        trusted_physical_addr: where to copy the TVM in confidential memory

    Returns:
        - Entry point address
        - List of steps
        - Current trusted offset
        - max gpa reached
    """
    inf = gdb.selected_inferior()
    steps = []
    with open(path, "rb") as f:
        elf = ELFFile(f)
        entry = elf.header["e_entry"]

        assert elf.header["e_machine"] == "EM_RISCV", "Not a RISCV ELF"
        print(f"Guest ELF Machine: {elf.header['e_machine']}")
        print(f"Guest Entry point (GPA): 0x{entry:X}")

        segments_info = []
        steps = []

        # Copy the ELF segments into Untrusted Memory
        for i, seg in enumerate(elf.iter_segments()):
            if seg.header.p_type != "PT_LOAD":
                continue

            gpa_addr = seg.header.p_paddr
            filesz = seg.header.p_filesz
            memsz = seg.header.p_memsz
            # Get the raw segment data
            seg_data = seg.data()

            # page-align the guest GPA for this segment
            guest_page = align_down(gpa_addr, PAGE_SIZE)
            page_offset = gpa_addr - guest_page

            # number of pages that cover this segment (including offset)
            num_pages = (page_offset + memsz + PAGE_SIZE - 1) // PAGE_SIZE

            # actual load base in untrusted memory must be page-aligned
            page_aligned_load_addr = untrusted_base_addr + guest_page

            # make sure the untrusted memory contains the full pages:
            #  - write the segment file bytes at page_aligned_load_addr + page_offset
            #  - zero out the rest of the covered pages
            total_bytes = num_pages * PAGE_SIZE
            # zero whole area first (simple, safe); then overwrite file bytes above
            zeros = bytes(total_bytes)
            inf.write_memory(page_aligned_load_addr, zeros)
            if filesz > 0:
                inf.write_memory(page_aligned_load_addr + page_offset, seg_data[:filesz])

            segments_info.append({
                "gpa_addr": gpa_addr,
                "guest_page": guest_page,
                "page_offset": page_offset,
                "load_page_addr": page_aligned_load_addr,
                "memsz": memsz,
                "filesz": filesz,
                "num_pages": num_pages,
                "index": i
            })

        # Step 2: Add measured pages for each segment
        current_trusted_offset = 0

        for s in segments_info:
            load_page_addr = s["load_page_addr"]
            guest_page = s["guest_page"]
            num_pages = s["num_pages"]

            # Trusted physical address for this segment
            trusted_addr = trusted_physical_addr + current_trusted_offset

            print(f"Segment {i}: filesz={filesz}, memsz={memsz}")
            print(f"  Source (untrusted): 0x{load_page_addr:x}")
            print(f"  Dest (trusted): 0x{trusted_addr:x}")
            print(f"  GPA: 0x{guest_page:x}")
            print(f"  Pages: {num_pages}")

            steps.append(
                Step(
                    name=f"add_tvm_measured_pages_{i}",
                    regs={
                        "a0": TVM_ID,
                        "a1": load_page_addr,  # Source: untrusted memory
                        "a2": trusted_addr,  # Dest: confidential memory
                        "a3": PAGE_SIZE_TO_ID[PAGE_SIZE],
                        "a4": num_pages,
                        "a5": guest_page,  # GPA to map at
                        "a6": (1 << 26) | (COVH_ADD_TVM_MEASURED_PAGES & 0xFFFF),
                        "a7": EID_COVH_ID,
                    },
                    setup_mem_fn=None,
                    assert_fn=ecall_ok
                ),
            )

            # Advance trusted memory pointer
            current_trusted_offset += num_pages * PAGE_SIZE

        return entry, steps, current_trusted_offset


def run() -> None:
    print("=== GDB Create TVM Program (from ELF) ===")

    # Create a single test domain
    domain_address: int = int(os.environ["BOOT_DOMAIN_ADDRESS"], 16)
    print(f"Test domain address 0x{domain_address:x}")

    domain = Domain(
            name="testdomain",
            instr_base=domain_address,
            data_base=domain_address + 0x1000
    )


    untrusted_ram_start: int = domain_address
    untrusted_ram_scratch: int = untrusted_ram_start + 0x1000
    untrusted_tvm_source_code: int = untrusted_ram_start + 0x2000
    confidential_ram_start: int = untrusted_ram_start + 0x4000
    trusted_tvm_state_start: int = confidential_ram_start + PAGE_DIRECTORY_SIZE
    trusted_tvm_ram_start: int = confidential_ram_start + PAGE_DIRECTORY_SIZE + TVM_VCPU_STATE_SIZE

    runner = Runner(commit_on_add=True)

    # Enumerate supervsisor domain: our untruste domain should discover the trusted domain
    # which contains a TSM with id 1
    runner.add_step(
        Step(
            name="enumerate_supervisor_domains",
            regs={
                "a0": 0,
                "a1": 0,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": SUPD_GET_ACTIVE_DOMAINS,
                "a7": EID_SUPD_ID,
            },
            setup_mem_fn=None,
            assert_fn=assert_get_active_domains,
        ),
        domain
    )

    # Get the TSM capabilities. The TSM will write its capabilities in a structure we provide
    runner.add_step(
        Step(
            name="get_tsm_info",
            regs={
                "a0": domain.data_base,
                "a1": 48,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_GET_TSM_INFO & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=assert_get_tsm_info,
        ),
        domain
    )

    # Donate pages to the untrusted domain. These will be used to host the TVM by the TSM hypervisor component
    # Each page has size 4k
    runner.add_step(
        Step(
            name="convert_pages",
            regs={
                "a0": confidential_ram_start,
                "a1": NUM_PAGES_TO_DONATE,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_CONVERT_PAGES & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=ecall_ok,
        ),
        domain
    )

    # Create a TVM object. The Host will tell where to create the GPT. The TSM will respond with the id
    # of the trusted VM
    runner.add_step(
        Step(
            name="create_tvm",
            regs={
                "a0": domain.data_base + 64,
                "a1": 16,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_CREATE_TVM & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=setup_create_tvm,
            setup_mem_args=[domain.data_base + 64, confidential_ram_start],
            assert_fn=assert_create_tvm,
        ),
        domain
    )

    runner.add_step(
        Step(
            name="add_tvm_memory_region",
            regs={
                "a0": TVM_ID,
                "a1": 0x0,  # Start of region (page-aligned)
                "a2": TVM_RAM_SIZE + 0x1000,  # Total size
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_ADD_MEMORY_REGION & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=ecall_ok,
        ),
        domain
    )

    guest_entry, steps, tvm_ram_off = load_guest_elf_and_make_steps(
        TVM_ELF_PATH, untrusted_tvm_source_code, trusted_tvm_ram_start
    )

    for step in steps:
        runner.add_step(step, domain)

    # map the rest as zero pages (stack, heap) as they don't are part of the measurement process
    runner.add_step(
        Step(
            name=f"add_tvm_zero_pages",
            regs={
                "a0": TVM_ID,
                "a1": trusted_tvm_ram_start + tvm_ram_off,  # Dest: confidential memory
                "a2": PAGE_SIZE_TO_ID[PAGE_SIZE],
                "a3": 1,
                "a4": 0x20000,  # GPA to map at
                "a5": 0,
                "a6": (1 << 26) | (COVH_ADD_TVM_ZERO_PAGES & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=ecall_ok
        ),
        domain
    )

    # Add vCPU with id=0 to the TVM
    runner.add_step(
        Step(
            name="create_vcpu",
            regs={
                "a0": TVM_ID,
                # vcpuid
                "a1": VCPU_ID,
                # tvm_state address (unused for now)
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_CREATE_TVM_VCPU & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=ecall_ok,
        ),
        domain
    )

    # Finalize the TVM. Provide GPA_BASE as the entrypoint
    runner.add_step(
        Step(
            name="finalize_tvm",
            regs={
                "a0": TVM_ID,
                "a1": guest_entry,
                # tvm identity addr (unused for now)
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_FINALIZE_TVM & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=ecall_ok,
        ),
        domain
    )

    # Start the TVM vCPU.
    runner.add_step(
        Step(
            name="run_tvm_vcpu",
            regs={
                "a0": TVM_ID,
                "a1": VCPU_ID,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_RUN_TVM_VCPU & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=ecall_ok,
        ),
        domain
    )

    runner.install_breakpoints()
    runner.debug_print()
    domain.debug_print()
    print("=== Payload and breakpoints installed; continue in GDB ===")


if __name__ == "__main__":
    run()
