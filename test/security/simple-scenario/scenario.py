dirpath = os.path.join(os.getcwd(), "test")
if dirpath not in sys.path:
    sys.path.insert(0, dirpath)

from riscv_tee import Step, Runner, Domain, read_mem


def run() -> None:
    print("=== GDB Get TSM Info Program ===")

    domain_address0: int = 0x82800_0000
    domain_address2: int = 0x83800_0000

    domain = Domain(
            name="domain0",
            instr_base=domain_address0,
            data_base=domain_address0 + 0x1000
    )

    trudy= Domain(
            name="domain2",
            instr_base=domain_address2,
            data_base=domain_address2 + 0x1000
    )

    runner = Runner(debug=True, commit_on_add=True)

    with open("attackgraph.csv", "r") as f:
        steps = get_steps_from_attackgrapth(f)

    runner.steps += steps
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
       trudy
    )

    domain.debug_print()
    trudy.debug_print()

    runner.debug_print()
   print("=== Payload and breakpoints installed; continue in GDB ===")


if __name__ == "__main__":
    run()
