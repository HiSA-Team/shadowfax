import os
import struct
import sys
import gdb
from typing import Dict, Optional

dirpath = os.path.join(os.getcwd(), "test")
if dirpath not in sys.path:
    sys.path.insert(0, dirpath)

from riscv_tee import Step, Runner, Domain, read_mem

def align_up(addr: int, align: int) -> int:
    return (addr + align - 1) & ~(align - 1)


def align_down(addr: int, align: int) -> int:
    return (addr) & ~(align - 1)

# ================ CoVE Constants ======================= #
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
COVH_CREATE_TVM_VCPU: int = 14
COVH_RUN_TVM_VCPU: int = 15

# ================ Create TVM Input ======================= #
PAGE_DIRECTORY_SIZE: int = 0x4000  # 16kib
GPA_BASE: int = 0x1000
NUM_PAGES_TO_DONATE: int = 16
JAL_LOOP_WORD = struct.pack("<I", 0x0000006F)  # jal x0, 0  -> tight infinite loop

TVM_ID: int = 1
VCPU_ID: int = 0
PAGE_SIZE_TO_ID = {0x1000: 0}
PAGE_SIZE: int = 0x1000  # 4096byte 4k



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
    a1 = regs["a1"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"
    assert a1 == TVM_ID, f"expected tvm_id={TVM_ID}in a1({a1})"

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


def setup_add_tvm_measured_pages(*args) -> None:
    tvm_source_code_addr = args[0]

    inf = gdb.selected_inferior()
    inf.write_memory(tvm_source_code_addr, JAL_LOOP_WORD)


def run() -> None:
    print("=== GDB Create TVM Program ===")

    # Create a single test domain
    domain_address: int = int(os.environ["BOOT_DOMAIN_ADDRESS"], 16)
    print(f"Test domain address 0x{domain_address:x}")

    domain = Domain(
            name="testdomain",
            instr_base=domain_address,
            data_base=domain_address + 0x1000
    )

    def ecall_ok(prev: Optional[Dict], curr: Dict) -> None:
        regs = curr["regs"]
        a0 = regs["a0"]
        assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"


    # Configure memory space of the domain
    tvm_source_code_addr: int = domain.data_base + 0x2000
    confidential_memory_start_addr: int = align_up(domain.data_base + 0x4000, PAGE_DIRECTORY_SIZE)
    tvm_page_start_addr: int = confidential_memory_start_addr + PAGE_DIRECTORY_SIZE + 0x1000

    confidential_memory_size: int = tvm_page_start_addr - confidential_memory_start_addr
    assert confidential_memory_size <= NUM_PAGES_TO_DONATE * 0x1000, (
        f"Insufficient memory region size: donated {NUM_PAGES_TO_DONATE * 0x1000} < needed {confidential_memory_size}"
        )

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
                "a0": confidential_memory_start_addr,
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
            setup_mem_args=[domain.data_base + 64, confidential_memory_start_addr],
            assert_fn=assert_create_tvm,
        ),
        domain
    )

    # Create a memory region. This region will be used to host the TVM code/data. The memory region
    # will be assigned to the TVM for the base GPA mapping
    runner.add_step(
        Step(
            name="add_tvm_memory_region",
            regs={
                "a0": TVM_ID,
                # Guest Physical Address (GPA)
                "a1": GPA_BASE,
                "a2": PAGE_SIZE,
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

    # Copy TVM code/data in the trusted domain. The TSM will map these pages into the TVM GPT
    # and measure them
    runner.add_step(
        Step(
            name="add_tvm_measured_pages",
            regs={
                "a0": TVM_ID,
                # will write at this address the loop jump loop instruction
                "a1": tvm_source_code_addr,
                # the address of the physical confidential memory
                # START_CONFIDENTIAL_REGION + 16 kb +
                "a2": tvm_page_start_addr,
                "a3": PAGE_SIZE_TO_ID[PAGE_SIZE],
                # num pages just one page
                "a4": 1,
                "a5": GPA_BASE,
                "a6": (1 << 26) | (COVH_ADD_TVM_MEASURED_PAGES & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=setup_add_tvm_measured_pages,
            setup_mem_args=[tvm_source_code_addr],
            assert_fn=ecall_ok,
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
                # entrypoint
                "a1": GPA_BASE,
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
