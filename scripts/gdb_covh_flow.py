import gdb
import struct
from dataclasses import dataclass
from typing import Callable, Dict, List, Optional, Any

# instruction words (little-endian bytes)
# https://www.cs.sfu.ca/~ashriram/Courses/CS295/assets/notebooks/RISCV/RISCV_CARD.pdf
ECALL_WORD = struct.pack("<I", 0x00000073)  # ecall
EBREAK_WORD = struct.pack("<I", 1 << 20 | 0x00000073)  # ebreak
NOP_WORD = struct.pack("<I", 0x00000013)  # addi x0,x0,0 -> nop
JAL_LOOP_WORD = struct.pack("<I", 0x0000006F)  # jal x0, 0  -> tight infinite loop


# --- helper snapshot helpers -------------------------------------------------
def read_reg(name: str) -> int:
    """Return integer value of a register (e.g. 'a0', 'pc')."""
    return int(gdb.parse_and_eval(f"${name}"))


def read_regs(names: List[str]) -> Dict[str, int]:
    return {n: read_reg(n) for n in names}


def read_mem(addr: int, size: int) -> bytes:
    """Read memory from selected inferior. Returns bytes."""
    inf = gdb.selected_inferior()
    return inf.read_memory(addr, size).tobytes()


@dataclass
class Step:
    name: str
    regs: Optional[Dict[str, int]] = None
    setup_mem_fn: Optional[Callable[[], None]] = None
    # assert_fn(prev_ctx, curr_ctx)
    assert_fn: Optional[Callable[[Optional[Dict[str, Any]], Dict[str, Any]], None]] = (
        None
    )


# --- PreBP / PostBP: before/after ECALL handling -----------------------------
class PreBP(gdb.Breakpoint):
    """
    Temporary breakpoint placed at the ECALL address.
    On hit it:
      - captures a 'prev' snapshot (regs+mem) and stores it in runner.pending_prev[step_index]
      - runs step.setup_mem_fn() and writes step.regs
      - installs a temporary PostBP at addr + 4 (the instruction after the ecall)
      - returns False so the inferior continues and executes the ECALL
    """

    def __init__(self, addr: int, step_index: int, runner: "TestRunner"):
        super().__init__(f"*0x{addr:x}", type=gdb.BP_BREAKPOINT, temporary=True)
        self.addr = addr
        self.step_index = step_index
        self.runner = runner

    def stop(self) -> bool:
        try:
            pc = int(gdb.parse_and_eval("$pc"))
        except Exception:
            return True  # be conservative and stop if we can't read pc

        if pc != self.addr:
            return False  # not our location, let other handlers deal with it

        step = self.runner.steps[self.step_index]

        # Allow step to set up memory/registers before the ECALL executes
        if step.regs:
            for reg, val in step.regs.items():
                gdb.execute(f"set ${reg} = {val}")

        if step.setup_mem_fn is not None:
            try:
                step.setup_mem_fn()
            except Exception as e:
                print(
                    f"  step[{self.step_index}] '{step.name}': setup_mem_fn exception: {e}"
                )
                # stop so the user can inspect if setup failed
                return True

        # Capture "prev" snapshot (state before ECALL executes)
        prev_snapshot = {"regs": read_regs(self.runner.regs_to_snapshot), "mem": {}}
        for label, (addr_reg, size) in self.runner.mem_snapshot_spec.items():
            if isinstance(addr_reg, str):
                addr = prev_snapshot["regs"].get(addr_reg, read_reg(addr_reg))
            else:
                addr = int(addr_reg)
            try:
                prev_snapshot["mem"][label] = read_mem(addr, size)
            except Exception as e:
                prev_snapshot["mem"][label] = f"<mem read failed: {e}>"

        # store pending prev snapshot for this step (consumed by PostBP)
        self.runner.pending_prev[self.step_index] = prev_snapshot

        # print registers for debugging
        gdb.execute(
            "info registers "
            + " ".join(self.runner.regs_to_snapshot)
            + " pc sepc scause stval mepc mcause mtval"
        )

        # return False so the inferior continues (ECALL instruction will execute)
        return False


class PostBP(gdb.Breakpoint):
    """
    Temporary breakpoint installed at addr+4 (the instruction after ECALL).
    On hit it:
      - captures the 'curr' snapshot (state after ECALL has been handled)
      - pulls the 'prev' saved by PreBP and calls step.assert_fn(prev, curr)
      - appends curr to runner.history for record-keeping
      - on assert failure, returns True so inferior stops for inspection
      - on success, returns False so the test continues to the next PreBP
    """

    def __init__(self, addr: int, step_index: int, runner: "TestRunner"):
        super().__init__(f"*0x{addr:x}", type=gdb.BP_BREAKPOINT, temporary=True)
        self.addr = addr
        self.step_index = step_index
        self.runner = runner

    def stop(self) -> bool:
        try:
            pc = int(gdb.parse_and_eval("$pc"))
        except Exception:
            return True

        if pc != self.addr:
            return False

        step = self.runner.steps[self.step_index]

        # snapshot current regs/mem: this is the post-ecall state
        curr_snapshot = {"regs": read_regs(self.runner.regs_to_snapshot), "mem": {}}
        for label, (addr_reg, size) in self.runner.mem_snapshot_spec.items():
            if isinstance(addr_reg, str):
                addr = curr_snapshot["regs"].get(addr_reg, read_reg(addr_reg))
            else:
                addr = int(addr_reg)
            try:
                curr_snapshot["mem"][label] = read_mem(addr, size)
            except Exception as e:
                curr_snapshot["mem"][label] = f"<mem read failed: {e}>"

        prev_snapshot = self.runner.pending_prev.pop(self.step_index, None)

        # call assertion if present
        if step.assert_fn is not None:
            try:
                step.assert_fn(prev_snapshot, curr_snapshot)
                print(f"  step[{self.step_index}] '{step.name}': assert PASS")
            except AssertionError as e:
                print(f"  step[{self.step_index}] '{step.name}': assert FAIL: {e}")
                # append curr for completeness then stop so user can inspect
                self.runner.history.append(curr_snapshot)
                return True
            except Exception as e:
                print(f"  step[{self.step_index}] '{step.name}': assert EXCEPTION: {e}")
                self.runner.history.append(curr_snapshot)
                return True

        # append the post-ecall snapshot to history
        self.runner.history.append(curr_snapshot)
        return True


# --- TestRunner --------------------------------------------------------------
class TestRunner:
    def __init__(
        self,
        payload_address: int,
    ):
        self.payload_address = payload_address
        self.steps: List[Step] = []
        self.history: List[Dict[str, Any]] = []
        self.pending_prev: Dict[int, Dict[str, Any]] = {}  # step_index -> prev snapshot
        self.regs_to_snapshot: List[str] = [
            "a0",
            "a1",
            "a2",
            "a3",
            "a4",
            "a5",
            "a6",
            "a7",
        ]
        # mapping label -> (addr_reg_or_int, size)
        self.mem_snapshot_spec: Dict[str, Any] = {}

    def add_step(self, step: Step) -> None:
        self.steps.append(step)

    def install_breakpoints(self, install_ebreak=False) -> None:
        """
        For each step:
         - write ECALL_WORD at payload_address + 8*i
         - write NOP_WORD at payload_address + 8*i + 4
         - install a temporary PreBP breakpoint at ecall addr
         - install a temporary PostBP breakpoint at nop addr

        After all steps:
         - write EBREAK_WORD after the ECALL/NOP "program"
         - write JAL_LOOP_WORD after the EBREAK_WORD to prevent the pc growing to infinite
        """
        inf = gdb.selected_inferior()
        # install step ecall words and PreBP breakpoints
        for i, step in enumerate(self.steps):
            ecall_addr = self.payload_address + 8 * i
            nop_addr = ecall_addr + 4

            inf.write_memory(ecall_addr, ECALL_WORD)
            print(f"Wrote ecall at 0x{ecall_addr:x}")
            inf.write_memory(nop_addr, NOP_WORD)
            print(f"Wrote nop at 0x{nop_addr:x}")

            PreBP(ecall_addr, i, self)
            print(f"Installed PreBP (step {i} - {step.name}) at 0x{ecall_addr:x}")

            PostBP(nop_addr, i, self)
            print(f"Installed PostBP (step {i} - {step.name}) at 0x{nop_addr:x}")

        # write an ebreak
        if install_ebreak:
            ebreak_addr = self.payload_address + 8 * len(self.steps)
            inf.write_memory(ebreak_addr, EBREAK_WORD)
            print(f"Wrote ebreak instruction at 0x{ebreak_addr:x}")

        # write infinite loop to ensure the program to hang
        loop_addr = self.payload_address + (8 + 1) * len(self.steps)
        inf.write_memory(loop_addr, JAL_LOOP_WORD)
        print(f"Wrote loop instruction at 0x{loop_addr:x}")
