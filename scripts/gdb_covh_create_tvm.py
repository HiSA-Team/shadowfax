import gdb
import os
import struct
from typing import Callable, Dict, List, Optional, Any


EID_COVH_ID: int = 0x434F5648

COVH_GET_TSM_INFO: int = 0
COVH_CONVERT_PAGES: int = 1
COVH_CREATE_TVM: int = 5
COVH_FINALIZE_TVM: int = 6
COVH_DESTROY_TVM: int = 8
COVH_CREATE_TVM_VCPU: int = 14
COVH_HOST_RUN_TVM_VCPU: int = 15


def assert_get_tsm_info() -> None:
    a0 = int(gdb.parse_and_eval("$a0"))
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"

    a1 = int(gdb.parse_and_eval("$a1"))
    assert a1 == 48, (
        f"a1 must contain the size of the TSMInfo struct 48bytes (current {a1})"
    )


def assert_create_tvm() -> None:
    a0 = int(gdb.parse_and_eval("$a0"))
    assert a0 == 0, f"ecall returned non-zero in a0 ({a0})"

    # for now hardcode the tvm id to 1
    a1 = int(gdb.parse_and_eval("$a1"))
    assert a1 == 1, f"a1 (containing tvm_id) should be 1; received ({a1})"


def assert_create_vcpu() -> None:
    a0 = int(gdb.parse_and_eval("$a0"))
    assert a0 == -1, f"ecall should not be implemented yet and returns -1({a0})"


def setup_create_tvm_params() -> None:
    pass


class PayloadBP(gdb.Breakpoint):
    def __init__(
        self,
        addr,
        name: Optional[str] = None,
        regs: Optional[Dict[str, int]] = None,
        setup_mem_fn: Optional[Callable[[], None]] = None,
        assert_fn: Optional[Callable[[], None]] = None,
    ):
        super().__init__(f"*0x{addr:x}", type=gdb.BP_BREAKPOINT, temporary=True)
        self.addr = addr
        self.name = name or f"payload@0x{addr:x}"
        self.regs = regs or {}
        self.setup_mem_fn = setup_mem_fn
        self.assert_fn = assert_fn

    def stop(self):
        # we are stopped safely here
        try:
            pc = int(gdb.parse_and_eval("$pc"))
        except Exception:
            return False

        if pc != self.addr:
            return False  # not our location

        if self.assert_fn is not None:
            try:
                self.assert_fn()
                print("  prev-check: PASS")
            except AssertionError as e:
                print(f"  prev-check: FAIL: {e}")
                # stop to allow user interaction on failure
                # return True -> stop in gdb so user can inspect
                return True
            except Exception as e:
                print(f"  prev-check raised unexpected exception: {e}")
                return True

        print(f"\n== PayloadBP handler: {self.name} ==")
        if self.setup_mem_fn is not None:
            self.setup_mem_fn()

        for reg in self.regs:
            gdb.execute(f"set ${reg} = {self.regs[reg]}")

        # single-step the ecall
        gdb.execute(f"info registers {' '.join(self.regs.keys())} pc sepc scause stval")

        # return True to stop and hand control back to the user after handler
        return True


def run() -> None:
    print("=== GDB Create TVM Program ===")
    payload_address: int = int(os.environ["SHADOWFAX_JUMP_ADDRESS"], 16)
    print(f"S-Mode address 0x{payload_address:x}")

    # write the ecall instruction to the address
    ecall_word = struct.pack("<I", 0x00000073)

    create_tvm_steps = [
        {
            "name": "get_tsm_info",
            "regs": {
                "a0": payload_address + 0x1000,
                "a1": 48,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": COVH_GET_TSM_INFO,
                "a7": EID_COVH_ID,
            },
            "setup_mem_fn": None,
            "assert_fn": None,
        },
        {
            "name": "convert_pages",
            "regs": {
                "a0": payload_address + 0x1000,
                "a1": 128,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": COVH_CONVERT_PAGES,
                "a7": EID_COVH_ID,
            },
            "setup_mem_fn": None,
            "assert_fn": assert_get_tsm_info,
        },
        {
            "name": "create_tvm",
            "regs": {
                "a0": payload_address + 0x1000,
                "a1": 16,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": COVH_CREATE_TVM,
                "a7": EID_COVH_ID,
            },
            "setup_mem_fn": None,
            "assert_fn": assert_get_tsm_info,
        },
        {
            "name": "create_vcpu",
            "regs": {
                "a0": payload_address + 0x1000,
                "a1": 48,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": COVH_CREATE_TVM_VCPU,
                "a7": EID_COVH_ID,
            },
            "setup_mem_fn": None,
            "assert_fn": assert_create_tvm,
        },
        {
            "name": "finalize_tvm",
            "regs": {
                "a0": payload_address + 0x1000,
                "a1": 48,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": COVH_FINALIZE_TVM,
                "a7": EID_COVH_ID,
            },
            "setup_mem_fn": None,
            "assert_fn": assert_create_vcpu,
        },
        {
            "name": "tvm_run",
            "regs": {
                "a0": payload_address + 0x1000,
                "a1": 48,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": COVH_HOST_RUN_TVM_VCPU,
                "a7": EID_COVH_ID,
            },
            "setup_mem_fn": None,
            "assert_fn": assert_get_tsm_info,
        },
    ]

    print(f"=== Starting COVH CREATE TVM FLOW (#{len(create_tvm_steps)} steps) ====")
    for i, step in enumerate(create_tvm_steps):
        print(f"Running step (#{i}) {step['name']}")

        # write the ecall instruction for each address of the test program.
        # We are creating a "dynamic program" which consists on "ecall"
        # instruction for each step. The Firmware increments the PC by 4 after
        # each ecall.
        address = payload_address + 4 * i
        gdb.selected_inferior().write_memory(address, ecall_word)

        # set a breakpoint to payload_address
        PayloadBP(
            address, step["name"], step["regs"], step["setup_mem_fn"], step["assert_fn"]
        )


if __name__ == "__main__":
    run()
