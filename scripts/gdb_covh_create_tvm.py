import os
import struct
import sys
import gdb
from typing import Dict, Optional

this_dir = os.path.dirname(__file__) or os.getcwd()
if this_dir not in sys.path:
    sys.path.insert(0, this_dir)

from gdb_covh_flow import Step, TestRunner, read_mem, read_reg


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


PAGE_DIRECTORY_SIZE: int = 0x4000  # 16kib
GPA_BASE: int = 0x1000
NUM_PAGES_TO_DONATE: int = 16
JAL_LOOP_WORD = struct.pack("<I", 0x0000006F)  # jal x0, 0  -> tight infinite loop

payload_address: int = int(os.environ["ROOT_DOMAIN_JUMP_ADDRESS"], 16)
confidential_memory_start_addr: int = payload_address + 0x4000
tvm_source_code_addr: int = payload_address + 0x2000
tvm_page_start_addr: int = confidential_memory_start_addr + PAGE_DIRECTORY_SIZE + 0x1000

confidential_memory_size: int = tvm_page_start_addr - confidential_memory_start_addr
assert confidential_memory_size <= NUM_PAGES_TO_DONATE * 0x1000, (
    f"Insufficient memory region size: donated {NUM_PAGES_TO_DONATE * 0x1000} < needed {confidential_memory_size}"
)


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
    assert tvm_vcpu_state_pages == 0, (
        f"tvm_vcpu_state_pages must be 0; current {tvm_vcpu_state_pages}"
    )


def assert_convert_pages(prev: Optional[Dict], curr: Dict) -> None:
    regs = curr["regs"]
    a0 = regs["a0"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"


def setup_create_tvm() -> None:
    # ecall parameter where to store the address
    tvm_params_addr = read_reg("a0")

    tvm_directory_addr = confidential_memory_start_addr
    tvm_state_addr = confidential_memory_start_addr + PAGE_DIRECTORY_SIZE

    print(f"tvm_params is at 0x{tvm_params_addr:x}")
    print(f"tvm_page_directory_addr (0x{tvm_params_addr:x}): 0x{tvm_directory_addr:x}")
    print(f"tvm_state_addr (0x{(tvm_params_addr + 8):x}): 0x{(tvm_state_addr):x}")

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


def assert_add_tvm_memory_region(prev: Optional[Dict], curr: Dict) -> None:
    regs = curr["regs"]
    a0 = regs["a0"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"


def setup_add_tvm_measured_pages() -> None:
    tvm_source_code_addr = read_reg("a1")

    inf = gdb.selected_inferior()
    inf.write_memory(tvm_source_code_addr, JAL_LOOP_WORD)


def assert_add_tvm_measured_pages(prev: Optional[Dict], curr: Dict) -> None:
    regs = curr["regs"]
    a0 = regs["a0"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"

    # assert that the "source code" has been copied successfully
    tvm_source_code_addr = prev["regs"]["a1"]

    instr_raw = read_mem(tvm_source_code_addr, 8)
    instr = struct.unpack("<I", instr_raw)[0]
    jal = struct.unpack("<I", JAL_LOOP_WORD)[0]

    assert instr == jal, (
        f"expected JAL_LOOP_WORD at {hex(tvm_source_code_addr)}; got {hex(instr)}"
    )
    # TODO: assert the pagetable mapping


def setup_create_tvm_vcpu() -> None:
    pass


def assert_create_tvm_vcpu(prev: Optional[Dict], curr: Dict) -> None:
    pass


def run() -> None:
    print("=== GDB Create TVM Program ===")
    print(f"S-Mode address 0x{payload_address:x}")

    runner = TestRunner(payload_address)

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
        )
    )

    runner.add_step(
        Step(
            name="get_tsm_info",
            regs={
                "a0": payload_address + 0x1000,
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
        )
    )

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
            assert_fn=assert_convert_pages,
        )
    )

    runner.add_step(
        Step(
            name="create_tvm",
            regs={
                "a0": payload_address + 0x1000,
                "a1": 16,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_CREATE_TVM & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=setup_create_tvm,
            assert_fn=assert_create_tvm,
        )
    )

    runner.add_step(
        Step(
            name="add_tvm_memory_region",
            regs={
                "a0": 1,
                # Guest Physical Address (GPA)
                "a1": GPA_BASE,
                "a2": 0x1000,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_ADD_MEMORY_REGION & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=assert_add_tvm_memory_region,
        )
    )

    runner.add_step(
        Step(
            name="add_tvm_measured_pages",
            regs={
                "a0": 1,
                # will write at this address the loop jump loop instruction
                "a1": tvm_source_code_addr,
                # the address of the physical confidential memory
                # START_CONFIDENTIAL_REGION + 16 kb +
                "a2": tvm_page_start_addr,
                # 0 for 4kb page
                "a3": 0,
                # num pages just one page
                "a4": 1,
                "a5": GPA_BASE,
                "a6": (1 << 26) | (COVH_ADD_TVM_MEASURED_PAGES & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=setup_add_tvm_measured_pages,
            assert_fn=None,
        )
    )

    runner.add_step(
        Step(
            name="create_vcpu",
            regs={
                "a0": payload_address + 0x1000,
                "a1": 48,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_CREATE_TVM_VCPU & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=None,
        )
    )

    runner.add_step(
        Step(
            name="finalize_tvm",
            regs={
                "a0": 1,
                "a1": GPA_BASE,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_FINALIZE_TVM & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=None,
        )
    )

    runner.add_step(
        Step(
            name="run_tvm_vcpu",
            regs={
                "a0": 1,
                "a1": 0,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": (1 << 26) | (COVH_RUN_TVM_VCPU & 0xFFFF),
                "a7": EID_COVH_ID,
            },
            setup_mem_fn=None,
            assert_fn=None,
        )
    )

    runner.install_breakpoints()
    print("=== Test harness ready; continue from gdb to run ===")


if __name__ == "__main__":
    run()
