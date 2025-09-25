import os
import struct
import sys
from typing import Dict, Optional

this_dir = os.path.dirname(__file__) or os.getcwd()
if this_dir not in sys.path:
    sys.path.insert(0, this_dir)

from gdb_covh_flow import Step, TestRunner, read_mem

EID_COVH_ID: int = 0x434F5648
COVH_GET_TSM_INFO: int = 0


def assert_get_tsm_info(prev: Optional[Dict], curr: Dict) -> None:
    # Example: ensure the ECALL returned successfully in a0 and left a1 = 48
    regs = curr["regs"]
    a0 = regs["a0"]
    a1 = regs["a1"]
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"
    assert a1 == 48, f"a1 must contain size 48 (current {a1})"

    assert prev is not None, "expecting the previous context not None"
    tsm_info_addr = prev["regs"]["a0"]
    tsm_state = read_mem(tsm_info_addr, 4)
    tsm_impl_id = read_mem(tsm_info_addr, 4)

    # read 8 bytes in one shot (state @ addr, impl id @ addr+4)
    raw = read_mem(tsm_info_addr, 8)
    if not isinstance(raw, (bytes, bytearray)) or len(raw) < 8:
        raise AssertionError(f"failed to read 8 bytes at {hex(tsm_info_addr)}: {raw!r}")

    tsm_state = struct.unpack("<I", raw[:4])[0]
    tsm_impl_id = struct.unpack("<I", raw[4:8])[0]

    assert tsm_state == 2, f"tsm_state must be 2; current {tsm_state}"
    assert tsm_impl_id == 69, f"tsm_impl_id must be 69; current {tsm_impl_id}"


def run() -> None:
    print("=== GDB Get TSM Info Program ===")
    payload_address: int = int(os.environ["SHADOWFAX_JUMP_ADDRESS"], 16)
    print(f"S-Mode address 0x{payload_address:x}")

    runner = TestRunner(payload_address)

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
