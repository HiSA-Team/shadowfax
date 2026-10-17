# Device trees

`base.dtsi` describes the hardware, RAM and firmware-owned staging areas. It
does not assign CPUs or memory to supervisor domains.

`default-domains.dtsi` provides the default trusted and untrusted domains,
their regions, the CPU assignment and the relocated FDT address. The root
`device-tree.dts` includes both files for `make firmware`.

Every bare-metal example has a `device-tree.dts`. It includes `base.dtsi` and
either defines its own domains or includes `default-domains.dtsi` and changes
what it needs. The standalone TVM launcher defines `monitor` and `runner`
domains directly; the multiple-domain test extends the default arrangement.

## Assigning the hart

OpenSBI decides which domain owns a hart from the **CPU node's** `opensbi-domain`
phandle (`lib/utils/fdt/fdt_domain.c`, `__fdt_parse_domain`). `possible-harts`
and `boot-hart` on the domain node only declare eligibility; neither moves the
hart. `default-domains.dtsi` therefore ends with:

```dts
&cpu0 {
	opensbi-domain = <&udomain>;
};
```

A tree that defines its own domains has to do the same for its boot domain, as
`test/standalone-tvm-launcher/device-tree.dts` does with `<&runner>`. It cannot
live in `base.dtsi`, because the owning domain differs per tree.

Leaving it out is silent: the hart stays in the root domain, so the launcher
runs with root's single all-memory `P:0x3f` region instead of its own, and
Shadowfax sees the caller as domain 0. Since `cove.rs` takes the caller's
identity from `sbi_hartindex_to_domain()`, every trust-map check then fails and
`GET_ACTIVE_DOMAINS` returns zero. Nothing catches this at build time yet.

`layout.py` exports domain entries and individual regions by DTS node name:

```make
SHADOWFAX_DOMAIN_RUNNER_ENTRY := ...
SHADOWFAX_REGION_RUNNER_RAM_BASE := ...
SHADOWFAX_REGION_RUNNER_RAM_SIZE := ...
```

Names become uppercase and punctuation becomes underscores. Domains can have
several separate RAM regions; the script does not select a host or guest pool.
`layout.json` records each domain's region membership and permissions.

Each example selects its launcher domain and two RAM regions before including
the shared launcher Makefile:

```make
LAUNCHER_DOMAIN     := RUNNER
LAUNCHER_RAM_REGION := RUNNER_RAM
GUEST_RAM_REGION    := GUEST_POOL

include $(ROOT)/guests/bare-metal/launcher.mk
```

These selectors use the exported spelling of the DTS node names, and
`launcher.mk` validates them at parse time against `SHADOWFAX_DOMAINS` and
`SHADOWFAX_RAM_REGIONS`, so a typo names the alternatives:

```
LAUNCHER_RAM_REGION='RUNNER_RMA' is not a RAM region declared by device-tree.dts;
available: GUEST_POOL MONITOR_RAM RUNNER_RAM
```

Launcher RAM holds code, stack and confidential metadata; guest RAM holds
donated guest pages. The regions do not have to be adjacent -- each is its own
`MEMORY` region in `launcher.ld`, which also asserts that the two do not
overlap and lets `ld` reject a guest larger than its region.

The example's DTB, exports and DTS dependency file are generated in its build
directory under `target/`. The platform's default TSM staging addresses remain
shared boot inputs; a domain staging another image gets its own
`SHADOWFAX_DOMAIN_<NAME>_TSM_IMAGE_ADDR` and signature address exports.
