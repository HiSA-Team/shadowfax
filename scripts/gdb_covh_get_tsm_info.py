import os
import struct
import sys
from typing import Dict, Optional

this_dir = os.path.dirname(__file__) or os.getcwd()
if this_dir not in sys.path:
    sys.path.insert(0, this_dir)

from gdb_covh_flow import Step, TestRunner, read_mem

EID_SUPD_ID: int = 0x53555044
EID_COVH_ID: int = 0x434F5648

SUPD_GET_ACTIVE_DOMAINS: int = 0

COVH_GET_TSM_INFO: int = 0


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
    #     _padding: u32: extra 32-bit because YES.
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
    assert tsm_version == 69, f"tsm_version must be 0; current  {tsm_version}"
    assert tsm_capabilities == 0, (
        f"tsm_capabilities must be 0; current {tsm_capabilities}"
    )
    assert tvm_state_pages == 1, f"tvm_state_pages must be 0; current {tvm_state_pages}"
    assert tvm_max_vcpus == 1, f"tvm_max_vcpus must be 1; current {tvm_max_vcpus}"
    assert tvm_vcpu_state_pages == 0, (
        f"tvm_vcpu_state_pages must be 0; current {tvm_vcpu_state_pages}"
    )


def run() -> None:
    print("=== GDB Get TSM Info Program ===")
    payload_address: int = int(os.environ["SHADOWFAX_JUMP_ADDRESS"], 16)
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

    runner.install_breakpoints()
    print("=== Test harness ready; continue from gdb to run ===")


if __name__ == "__main__":
    run()
