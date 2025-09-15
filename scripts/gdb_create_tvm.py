import gdb
import os
import struct
from typing import Callable, Dict, List, Optional, Any


class PayloadBP(gdb.Breakpoint):
    def __init__(
        self,
        addr,
        name: Optional[str] = None,
        regs: Optional[Dict[str, int]] = None,
        setup_mem: Optional[Callable[[gdb.Inferior], None]] = None,
    ):
        super().__init__(f"*0x{addr:x}", type=gdb.BP_BREAKPOINT, temporary=True)
        self.addr = addr
        self.name = name or f"payload@0x{addr:x}"
        self.regs = regs or {}
        self.setup_mem = setup_mem

    def stop(self):
        # we are stopped safely here
        try:
            pc = int(gdb.parse_and_eval("$pc"))
        except Exception:
            return False

        if pc != self.addr:
            return False  # not our location

        inferior = gdb.selected_inferior()

        print(f"\n== PayloadBP handler: {self.name} ==")
        if self.setup_mem is not None:
            self.setup_mem(inferior)

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
    gdb.selected_inferior().write_memory(payload_address, ecall_word)

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
                "a6": 0,
                "a7": 0x434F5648,
            },
            "setup_mem": None,
        },
        {
            "name": "create_tvm",
            "regs": {
                "a0": payload_address + 0x1000,
                "a1": 48,
                "a2": 0,
                "a3": 0,
                "a4": 0,
                "a5": 0,
                "a6": 5,
                "a7": 0x434F5648,
            },
            "setup_mem": None,
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
                "a6": 6,
                "a7": 0x434F5648,
            },
            "setup_mem": None,
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
                "a6": 7,
                "a7": 0x434F5648,
            },
            "setup_mem": None,
        },
    ]

    print(f"=== Starting COVH CREATE TVM FLOW (#{len(create_tvm_steps)} steps) ====")
    for i, step in enumerate(create_tvm_steps):
        print(f"Running step (#{i}) {step['name']}")
        # set a breakpoint to payload_address
        PayloadBP(payload_address, step["name"], step["regs"], step["setup_mem"])
        # gdb.execute("continue")


if __name__ == "__main__":
    run()
