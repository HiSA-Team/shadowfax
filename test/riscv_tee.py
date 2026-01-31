"""
RISC-V Confidential Domains Attack Runner for GDB (domain-owned steps)

This module provides a flexible framework to build tiny ECALL "programs"
inside isolated per-domain instruction regions and to instrument their
execution from GDB using temporary breakpoints.

Key architectural change
- Domains now own their own sequences of Steps (each domain represents an
  independent "program" area). The Runner manages a collection of Domain
  objects and orchestrates writes, breakpoint installation and snapshotting.
  This makes it natural to represent multi-domain scenarios where each
  domain contains independent ECALL sequences and data.

High-level behavior
- Create Domain objects that describe an instruction base and data base.
- Create Step objects (name, optional regs, setup_mem_fn, assert_fn).
- Add domains to a Runner (runner.add_domain(domain)) or let runner add a
  domain automatically when add_step(domain, step) is called.
- Use runner.add_step(step, domain) to allocate an instruction slot in the
  target domain and schedule the ECALL/NOP and domain tail writes.
- Use runner.install_breakpoints() to install temporary PreBP/PostBP
  breakpoints around each ECALL (requires GDB). PreBP captures a "prev"
  snapshot, runs step setup, installs a PostBP at pc+4; PostBP captures
  "curr" snapshot and runs the step.assert_fn(prev, curr).
- Writes may be performed immediately (if a GDB inferior is present and the
  runner is configured to commit on add) or recorded as planned_writes for
  later commit via commit_planned_writes().

Usage (interactive in GDB)
1. (gdb) source /path/to/riscv_tee.py
2. Create Domain(s) and Step(s). Add domains to Runner or pass domain to
   runner.add_step(step, domain). The call will allocate a slot and schedule
   ECALL/NOP and a domain tail loop; writes are attempted immediately if
   possible otherwise stored as planned writes.
3. Optionally call runner.install_breakpoints() to install PreBP/PostBP
   temporary breakpoints around each ECALL. Call runner.remove_breakpoints()
   to delete them. The written code remains in memory regardless.
4. Continue the inferior: breakpoints capture snapshots, run setup/assert,
   and either stop the inferior on failures or allow it to proceed.

API
- Domain(name, instr_base, data_base)
- Runner(commit_on_add=True)
- runner.add_domain(domain)
- runner.add_step(step, domain)
- runner.install_breakpoints()
- runner.remove_breakpoints()
- runner.commit_planned_writes()
- runner.debug_print()  # prints domains and their steps

Notes and caveats
- This script depends on GDB's Python API when used interactively.

Author: Giuseppe Capasso <capassog97@gmail.com>
"""

import struct
from dataclasses import dataclass
from typing import Callable, Dict, List, Optional, Any, Tuple

# Try to import gdb. If not available, operate in "offline" mode where memory
# writes are recorded and can be committed when GDB is present.
try:
    import gdb  # type: ignore
    GDB_AVAILABLE = True
except Exception:
    gdb = None  # type: ignore
    GDB_AVAILABLE = False

# instruction words (little-endian bytes)
# https://www.cs.sfu.ca/~ashriram/Courses/CS295/assets/notebooks/RISCV/RISCV_CARD.pdf
ECALL_WORD = struct.pack("<I", 0x00000073)  # ecall
EBREAK_WORD = struct.pack("<I", 1 << 20 | 0x00000073)  # ebreak
NOP_WORD = struct.pack("<I", 0x00000013)  # addi x0,x0,0 -> nop
JAL_LOOP_WORD = struct.pack("<I", 0x0000006F)  # jal x0, 0  -> tight infinite loop


# ------------------------------ helper snapshot helpers -------------------------------------------
def read_reg(name: str) -> int:
    if not GDB_AVAILABLE:
        raise RuntimeError("gdb not available")
    return int(gdb.parse_and_eval(f"${name}"))


def read_regs(names: List[str]) -> Dict[str, int]:
    if not GDB_AVAILABLE:
        raise RuntimeError("gdb not available")
    return {n: read_reg(n) for n in names}


def read_mem(addr: int, size: int) -> bytes:
    if not GDB_AVAILABLE:
        raise RuntimeError("gdb not available")
    inf = gdb.selected_inferior()
    return inf.read_memory(addr, size).tobytes()


def write_mem(addr: int, data: bytes) -> None:
    if not GDB_AVAILABLE:
        raise RuntimeError("gdb not available")
    inf = gdb.selected_inferior()
    inf.write_memory(addr, data)


# -------------------------------------- datatypes -----------------------------------------

@dataclass
class Step:
    name: str
    regs: Optional[Dict[str, int]] = None
    setup_mem_fn: Optional[Callable[[], None]] = None
    setup_mem_args: Optional[[]] =  None
    # assert_fn(prev_ctx, curr_ctx)
    assert_fn: Optional[Callable[[Optional[Dict[str, Any]], Dict[str, Any]], None]] = (
        None
    )
    ecall_addr: Optional[int] = None


@dataclass
class PlannedWrite:
    addr: int
    data: bytes


@dataclass
class Domain:
    """Represents a domain with separated instruction and data regions.

    Each Domain now owns a list of Steps (its independent program).
    """

    name: str
    instr_base: int
    data_base: int

    # bookkeeping
    _instr_slots: int = 0  # each slot is 8 bytes: ECALL + NOP

    def __post_init__(self):
        self.instructions = []  # optional free-form bookkeeping
        self.data = {}
        self.steps: List[Step] = []

    def allocate_instr_slot(self) -> int:
        slot_addr = self.instr_base + 8 * self._instr_slots
        self._instr_slots += 1
        return slot_addr

    def debug_print(self) -> None:
        print(f"Domain '{self.name}': instr_base={hex(self.instr_base)}, data_base={hex(self.data_base)}, slots={self._instr_slots}")


# --------------------------- PreBP / PostBP: before/after ECALL handling ------------------------
if GDB_AVAILABLE:
    class PreBP(gdb.Breakpoint):
        """
        Temporary breakpoint placed at the ECPteLeafPerms::RWXALL address.

        On hit it:
          - captures a 'prev' snapshot (regs+mem) and stores it in runner.pending_prev[(didx,sidx)]
          - runs step.setup_mem_fn() and writes step.regs
          - installs a temporary PostBP at addr + 4 (the instruction after the ecall)
          - returns False so the inferior continues and executes the ECALL
        """

        def __init__(self, addr: int, domain_index: int, step_index: int, runner: "Runner"):
            super().__init__(f"*0x{addr:x}", type=gdb.BP_BREAKPOINT, temporary=True)
            self.addr = addr
            self.domain_index = domain_index
            self.step_index = step_index
            self.runner = runner

        def stop(self) -> bool:
            try:
                pc = int(gdb.parse_and_eval("$pc"))
            except Exception:
                return True  # be conservative and stop if we can't read pc

            if pc != self.addr:
                return False  # not our location, let other handlers deal with it

            step = self.runner.domains[self.domain_index].steps[self.step_index]

            # Allow step to set up memory/registers before the ECALL executes
            if step.regs:
                for reg, val in step.regs.items():
                    gdb.execute(f"set ${reg} = {val}")

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
            self.runner.pending_prev[(self.domain_index, self.step_index)] = prev_snapshot

            # print registers for debugging
            try:
                gdb.execute(
                    "info registers "
                    + " ".join(self.runner.regs_to_snapshot)
                    + " pc sepc scause stval mepc mcause mtval"
                )
            except Exception:
                pass

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
          - on success, returns True to stop (same as previous behavior)
        """

        def __init__(self, addr: int, domain_index: int, step_index: int, runner: "Runner"):
            super().__init__(f"*0x{addr:x}", type=gdb.BP_BREAKPOINT, temporary=True)
            self.addr = addr
            self.domain_index = domain_index
            self.step_index = step_index
            self.runner = runner

        def stop(self) -> bool:
            try:
                pc = int(gdb.parse_and_eval("$pc"))
            except Exception:
                return True

            if pc != self.addr:
                return False

            step = self.runner.domains[self.domain_index].steps[self.step_index]

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

            prev_snapshot = self.runner.pending_prev.pop((self.domain_index, self.step_index), None)

            # call assertion if present
            if step.assert_fn is not None:
                try:
                    step.assert_fn(prev_snapshot, curr_snapshot)
                    print(f"  domain[{self.domain_index}].step[{self.step_index}] '{step.name}': assert PASS")
                except AssertionError as e:
                    print(f"  domain[{self.domain_index}].step[{self.step_index}] '{step.name}': assert FAIL: {e}")
                    # append curr for completeness then stop so user can inspect
                    self.runner.history.append({
                        "domain_index": self.domain_index,
                        "step_index": self.step_index,
                        "step_name": step.name,
                        "snapshot": curr_snapshot,
                    })
                    return True
                except Exception as e:
                    print(f"  domain[{self.domain_index}].step[{self.step_index}] '{step.name}': assert EXCEPTION: {e}")
                    self.runner.history.append({
                        "domain_index": self.domain_index,
                        "step_index": self.step_index,
                        "step_name": step.name,
                        "snapshot": curr_snapshot,
                    })
                    return True

            # append the post-ecall snapshot to history
            self.runner.history.append({
                "domain_index": self.domain_index,
                "step_index": self.step_index,
                "step_name": step.name,
                "snapshot": curr_snapshot,
            })
            return True


# ------------------------ Runner ------------------------
class Runner:
    """Runner that manages multiple domains (each with its own Steps).

    Parameters
    - debug: if True, install PreBP breakpoints when steps are added so the
             harness runs immediately under gdb. If False, breakpoints are
             installed only when install_breakpoints() is explicitly called.
    - commit_on_add: if True attempt to write ECALL/NOP into the inferior when
             add_step is called; otherwise record planned writes for later commit.
    """

    def __init__(self, commit_on_add: bool = True):
        self.commit_on_add = commit_on_add

        self.domains: List[Domain] = []
        self.history: List[Dict[str, Any]] = []
        # keyed by (domain_index, step_index)
        self.pending_prev: Dict[Tuple[int, int], Dict[str, Any]] = {}

        # snapshot config
        self.regs_to_snapshot: List[str] = ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7"]
        self.mem_snapshot_spec: Dict[str, Tuple[Any, int]] = {}

        # planned writes when we cannot write immediately
        self.planned_writes: List[PlannedWrite] = []

        # keep references to installed PreBP/PostBP objects so we can delete them
        self._installed_bps: List[gdb.Breakpoint] = [] if GDB_AVAILABLE else []

    def add_domain(self, domain: Domain) -> int:
        """Add a domain to the runner. Returns the domain index."""
        if domain in self.domains:
            return self.domains.index(domain)
        self.domains.append(domain)
        return len(self.domains) - 1

    def _schedule_write(self, addr: int, data: bytes) -> None:
        try:
            if self.commit_on_add and GDB_AVAILABLE:
                write_mem(addr, data)
            else:
                raise RuntimeError("deferred")
        except RuntimeError:
            # record planned write for later commit
            self.planned_writes.append(PlannedWrite(addr=addr, data=data))

    def commit_planned_writes(self) -> List[Tuple[int, bool, Optional[str]]]:
        results: List[Tuple[int, bool, Optional[str]]] = []
        if not GDB_AVAILABLE:
            raise RuntimeError("gdb not available; cannot commit planned writes")
        remaining: List[PlannedWrite] = []
        for pw in self.planned_writes:
            try:
                write_mem(pw.addr, pw.data)
                results.append((pw.addr, True, None))
            except Exception as e:
                results.append((pw.addr, False, str(e)))
                remaining.append(pw)
        self.planned_writes = remaining
        return results

    def add_step(self, step: Step, domain: Domain) -> None:
        """Allocate instruction slot in the given domain, write ECALL/NOP and
        update domain tail. If debug mode is enabled, install a PreBP for this
        step.

        If the domain is not already known to the runner, it will be added.
        """
        # ensure domain is registered
        if domain not in self.domains:
            self.add_domain(domain)
        dindex = self.domains.index(domain)

        ecall_addr = domain.allocate_instr_slot()
        step.ecall_addr = ecall_addr

        # schedule instruction writes
        self._schedule_write(ecall_addr, ECALL_WORD)
        self._schedule_write(ecall_addr + 4, NOP_WORD)

        # update domain tail (JAL_LOOP) after the last slot
        tail_addr = domain.instr_base + 8 * domain._instr_slots
        self._schedule_write(tail_addr, JAL_LOOP_WORD)

        # append the step to the domain
        domain.steps.append(step)
        sindex = len(domain.steps) - 1

        # execute the setup function
        if step.setup_mem_fn is not None:
            try:
                step.setup_mem_fn(*(step.setup_mem_args))
            except Exception as e:
                print(f"  domain[{self.domain_index}].step[{self.step_index}] '{step.name}': setup_mem_fn exception: {e}")

    def install_breakpoints(self) -> None:
        if not GDB_AVAILABLE:
            raise RuntimeError("gdb not available; cannot install breakpoints")
        # remove any breakpoints we installed  earlier
        self.remove_breakpoints()
        for dindex, domain in enumerate(self.domains):
            for sindex, step in enumerate(domain.steps):
                if step.ecall_addr is None:
                    continue
                # create a breakpoint on ECALL instruction
                bp = PreBP(step.ecall_addr, dindex, sindex, self)
                self._installed_bps.append(bp)
                # Create a breakpoint on the NOP
                bp = PostBP(step.ecall_addr + 4, dindex, sindex, self)
                self._installed_bps.append(bp)

    def remove_breakpoints(self) -> None:
        if not GDB_AVAILABLE:
            self._installed_bps = []
            return
        for bp in list(self._installed_bps):
            try:
                bp.delete()
            except Exception:
                pass
        self._installed_bps = []

    # convenience utility to dump runner state
    def debug_print(self) -> None:
        print("Runner state:")
        for dindex, domain in enumerate(self.domains):
            print(f" Domain[{dindex}] '{domain.name}': instr_base={hex(domain.instr_base)}, data_base={hex(domain.data_base)}, slots={domain._instr_slots}")
            for sindex, step in enumerate(domain.steps):
                print(f"   Step[{dindex},{sindex}] '{step.name}' @ {hex(step.ecall_addr) if step.ecall_addr else None}")

        if len(self.planned_writes) > 0:
            print("Planned writes:")
            for pw in self.planned_writes:
                print(f"  {hex(pw.addr)}: {pw.data.hex()}")
