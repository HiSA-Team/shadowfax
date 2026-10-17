#!/usr/bin/env python3
"""Compile a platform device tree, check its memory map, and export the
addresses the build needs back out of it.

The device tree stays the interchange format -- OpenSBI's fdt_domain.c and
Shadowfax's platform.rs both parse it -- so this is not a new description
format. It is a single producer for numbers that were previously re-derived
with fdtget in every Makefile, linker script and C header.

Run it after `make tsm`, so the staged TSM image and signature exist: their
sizes are not known until they are built, and the tree carries them.

    layout.py platform/generic/device-tree.dts target/platform/generic

Writes device-tree.dtb plus layout.{mk,sh,h,json} into the output directory.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass

import libfdt

# Shadowfax programs one PMP entry per region; mirrors platform.rs.
MAX_MEMORY_REGIONS = 16

# QEMU's RISC-V boot.c aligns the DTB start down after subtracting its size.
QEMU_DTB_ALIGNMENT = 2 << 20

# <address, size> properties whose size cell is filled in from a staged file.
ARTIFACTS = {"tsm-image": "tsm.elf", "tsm-signature": "tsm.bin.signature"}


# --------------------------------------------------------------------------
# Small helpers over libfdt
# --------------------------------------------------------------------------


def subnodes(fdt, offset):
    node = fdt.first_subnode(offset, libfdt.QUIET_NOTFOUND)
    while node >= 0:
        yield node
        node = fdt.next_subnode(node, libfdt.QUIET_NOTFOUND)


def raw(fdt, offset, name):
    """Property bytes, or None when the property is absent."""
    value = fdt.getprop(offset, name, libfdt.QUIET_NOTFOUND)
    return None if isinstance(value, int) else bytes(value)


def present(fdt, offset, name):
    return raw(fdt, offset, name) is not None


def cells(fdt, offset, name):
    value = raw(fdt, offset, name)
    if value is None:
        return []
    return [int.from_bytes(value[i : i + 4], "big") for i in range(0, len(value) - 3, 4)]


def address(fdt, offset, name):
    """Decode the leading two-cell address of a property."""
    words = cells(fdt, offset, name)
    if len(words) < 2:
        sys.exit("layout.py: %s/%s is not a 64-bit address" % (fdt.get_path(offset), name))
    return (words[0] << 32) | words[1]


def ranges(fdt, offset, name="reg"):
    """Decode a property of <address, size> pairs, two cells each."""
    words = cells(fdt, offset, name)
    return [
        ((words[i] << 32) | words[i + 1], (words[i + 2] << 32) | words[i + 3])
        for i in range(0, len(words) - 3, 4)
    ]


def compatible(fdt, offset):
    value = raw(fdt, offset, "compatible")
    return [] if value is None else [s.decode() for s in value.split(b"\0") if s]


def name_of(fdt, offset):
    return fdt.get_name(offset).split("@")[0]


# --------------------------------------------------------------------------
# Model
# --------------------------------------------------------------------------


@dataclass
class Region:
    name: str
    base: int
    order: int
    mmio: bool

    @property
    def size(self):
        return 1 << self.order

    @property
    def end(self):
        return self.base + self.size


@dataclass
class Domain:
    name: str
    offset: int
    is_tsm: bool
    boot: bool
    next_addr: int
    regions: list
    permissions: list
    trusts: list


class Platform:
    def __init__(self, fdt):
        self.fdt = fdt
        self.shadowfax = self._offset("/chosen/shadowfax")
        domains = self._offset("/chosen/opensbi-domains")

        self.memory = []
        for node in subnodes(fdt, 0):
            if raw(fdt, node, "device_type") == b"memory\0":
                self.memory += ranges(fdt, node)
        if not self.memory:
            sys.exit("layout.py: the tree has no /memory node")

        self.reserved = []
        node = fdt.subnode_offset(0, "reserved-memory", libfdt.QUIET_NOTFOUND)
        if node >= 0:
            for child in subnodes(fdt, node):
                if present(fdt, child, "no-map"):
                    for base, size in ranges(fdt, child):
                        self.reserved.append((name_of(fdt, child), base, size))

        self.regions = []
        by_phandle = {}
        for node in subnodes(fdt, domains):
            if "opensbi,domain,memregion" not in compatible(fdt, node):
                continue
            region = Region(
                name=name_of(fdt, node),
                base=address(fdt, node, "base"),
                order=cells(fdt, node, "order")[0],
                mmio=present(fdt, node, "mmio"),
            )
            self.regions.append(region)
            by_phandle[fdt.get_phandle(node)] = region

        self.domains = []
        self.domain_by_phandle = {}
        for node in subnodes(fdt, domains):
            if "opensbi,domain,instance" not in compatible(fdt, node):
                continue
            words = cells(fdt, node, "regions")
            domain = Domain(
                name=name_of(fdt, node),
                offset=node,
                is_tsm=present(fdt, node, "shadowfax,tsm"),
                boot=present(fdt, node, "boot-hart"),
                next_addr=address(fdt, node, "next-addr") if present(fdt, node, "next-addr") else 0,
                regions=[by_phandle[words[i]] for i in range(0, len(words) - 1, 2)],
                permissions=words[1::2],
                trusts=cells(fdt, node, "shadowfax,trusts"),
            )
            self.domains.append(domain)
            self.domain_by_phandle[fdt.get_phandle(node)] = domain

        cpus = self._offset("/cpus")
        self.smp = sum(
            1 for node in subnodes(fdt, cpus) if raw(fdt, node, "device_type") == b"cpu\0"
        )

    def _offset(self, path):
        offset = self.fdt.path_offset(path, libfdt.QUIET_NOTFOUND)
        if offset < 0:
            sys.exit("layout.py: the tree has no %s node" % path)
        return offset

    @property
    def ram(self):
        return min(b for b, _ in self.memory), max(b + s for b, s in self.memory)

# --------------------------------------------------------------------------
# Artifact sizes
# --------------------------------------------------------------------------


def fill_artifact_sizes(fdt, artifacts):
    """Set the size cell of every <address, size> property naming a staged file.

    A signed image has no size until it is built, which is why the build used
    to patch the blob with fdtput afterwards -- including from `make run`,
    which rewrote a staged artifact on every run.
    """
    # The platform default lives on /chosen/shadowfax; a domain that stages its
    # own TSM carries the same pair under a shadowfax, prefix.
    targets = [(fdt.path_offset("/chosen/shadowfax"), key) for key in ARTIFACTS]
    for node in subnodes(fdt, fdt.path_offset("/chosen/opensbi-domains")):
        targets += [(node, "shadowfax," + key) for key in ARTIFACTS]

    filled = {}
    for node, key in targets:
        value = raw(fdt, node, key)
        if value is None or len(value) != 16:
            continue
        path = os.path.join(artifacts, ARTIFACTS[key.rpartition(",")[2]])
        if not os.path.exists(path):
            sys.exit("layout.py: %s is missing; run `make tsm` first" % path)
        size = os.path.getsize(path)
        fdt.setprop(node, key, value[:8] + size.to_bytes(8, "big"))
        filled["%s/%s" % (fdt.get_path(node), key)] = size
    return filled


# --------------------------------------------------------------------------
# Checks
# --------------------------------------------------------------------------


def problems(platform, filled):
    found = []
    fdt = platform.fdt
    ram_base, ram_end = platform.ram
    ram_regions = [r for r in platform.regions if not r.mmio]

    # PMP uses NAPOT encoding (base | ones), so a region that is not aligned to
    # its own size protects a different range than the tree declares, and
    # validate_convert_range() in state.rs refuses to convert it.
    for region in platform.regions:
        if region.order >= 64:
            found.append("region %s has unsupported order %d" % (region.name, region.order))
        elif not region.mmio and region.base % region.size:
            found.append(
                "region %s at %#x is not aligned to its size %#x (PMP NAPOT)"
                % (region.name, region.base, region.size)
            )

    ordered = sorted(ram_regions, key=lambda r: r.base)
    for lower, upper in zip(ordered, ordered[1:]):
        if lower.end > upper.base:
            found.append(
                "regions %s (%#x-%#x) and %s (%#x-%#x) overlap"
                % (lower.name, lower.base, lower.end, upper.name, upper.base, upper.end)
            )

    for region in ram_regions:
        if region.base < ram_base or region.end > ram_end:
            found.append(
                "region %s (%#x-%#x) is outside /memory (%#x-%#x)"
                % (region.name, region.base, region.end, ram_base, ram_end)
            )

    # Shadowfax's private staging must not sit in memory a supervisor owns.
    for name, base, size in platform.reserved:
        if base < ram_base or base + size > ram_end:
            found.append("reserved %s (%#x-%#x) is outside /memory" % (name, base, base + size))
        for region in ram_regions:
            if base < region.end and region.base < base + size:
                found.append(
                    "reserved %s (%#x-%#x) overlaps supervisor region %s (%#x-%#x)"
                    % (name, base, base + size, region.name, region.base, region.end)
                )

    for location, size in sorted(filled.items()):
        path, key = location.rsplit("/", 1)
        base = address(fdt, fdt.path_offset(path), key)
        if not any(base >= b and base + size <= b + s for _, b, s in platform.reserved):
            found.append(
                "%s (%#x-%#x, %d bytes) is not inside a no-map reservation"
                % (location, base, base + size, size)
            )

    highest = max(
        [r.end for r in ram_regions] + [b + s for _, b, s in platform.reserved] + [ram_base]
    )
    dtb_size = fdt.totalsize()
    dtb_start = (ram_end - dtb_size) & ~(QEMU_DTB_ALIGNMENT - 1)
    if highest > dtb_start:
        found.append(
            "memory map ends at %#x, past QEMU DTB start %#x "
            "(%d-byte DTB, %#x alignment)"
            % (highest, dtb_start, dtb_size, QEMU_DTB_ALIGNMENT)
        )

    # program_domain_pmp() loads a TSM's regions together with those of the
    # domain it acts for, so the worst case is the sum of the pair.
    for domain in platform.domains:
        if len(domain.regions) > MAX_MEMORY_REGIONS:
            found.append(
                "domain %s declares %d regions (max %d)"
                % (domain.name, len(domain.regions), MAX_MEMORY_REGIONS)
            )
        if not domain.is_tsm:
            continue
        for phandle in domain.trusts:
            other = platform.domain_by_phandle.get(phandle)
            if other and len(domain.regions) + len(other.regions) > MAX_MEMORY_REGIONS:
                found.append(
                    "%s + %s need %d PMP entries during bootstrap (max %d)"
                    % (
                        domain.name,
                        other.name,
                        len(domain.regions) + len(other.regions),
                        MAX_MEMORY_REGIONS,
                    )
                )
    return found


# --------------------------------------------------------------------------
# Export
# --------------------------------------------------------------------------


def ident(name):
    return "".join(c if c.isalnum() else "_" for c in name).upper()


def human(size):
    # The result feeds QEMU's -m, which reads a suffix-less number as mebibytes,
    # so every branch has to carry an explicit unit.
    for suffix, scale in (("G", 1 << 30), ("M", 1 << 20), ("K", 1 << 10)):
        if size >= scale and size % scale == 0:
            return "%d%s" % (size // scale, suffix)
    return "%dB" % size


def exports(platform):
    fdt = platform.fdt
    ram_base, ram_end = platform.ram
    values = {
        "RAM_BASE": ram_base,
        "RAM_SIZE": ram_end - ram_base,
        "DICE_ADDR": address(fdt, platform.shadowfax, "dice-input"),
        "FDT_ADDR": address(fdt, platform.shadowfax, "host-fdt"),
    }
    for key in ARTIFACTS:
        if present(fdt, platform.shadowfax, key):
            values[ident(key) + "_ADDR"] = address(fdt, platform.shadowfax, key)
    # A domain may stage its own TSM instead of using the platform default.
    for domain in platform.domains:
        prefix = "DOMAIN_" + ident(domain.name)
        values[prefix + "_ENTRY"] = domain.next_addr
        for key in ARTIFACTS:
            prop = "shadowfax," + key
            if present(fdt, domain.offset, prop):
                values["%s_%s_ADDR" % (prefix, ident(key))] = address(
                    fdt, domain.offset, prop
                )

    regions = {r.name: r for r in platform.regions}
    # Plain counters, kept out of the address table so they are not hex-formatted.
    flags = {
        "SMP": platform.smp,
    }
    return values, flags, regions


def write_assignments(path, values, flags, regions, platform, separator):
    """Make and shell consume the same variables, with different assignment syntax."""
    ram_base, ram_end = platform.ram
    variables = {key: hex(value) for key, value in sorted(values.items())}
    variables.update((key, str(value)) for key, value in sorted(flags.items()))
    variables["QEMU_MEM"] = human(ram_end - ram_base)
    # The selector names a launcher may use, so a typo can be reported against
    # what the tree actually declares instead of expanding to an empty defsym.
    variables["REGIONS"] = " ".join(sorted(ident(name) for name in regions))
    # A launcher can only sit in, or hand out, real memory.
    variables["RAM_REGIONS"] = " ".join(
        sorted(ident(name) for name, r in regions.items() if not r.mmio)
    )
    variables["DOMAINS"] = " ".join(sorted(ident(d.name) for d in platform.domains))
    for name, region in sorted(regions.items()):
        variables["REGION_%s_BASE" % ident(name)] = hex(region.base)
        variables["REGION_%s_SIZE" % ident(name)] = hex(region.size)
    # Make takes a bare value; sh would split a multi-word one across the
    # assignment and run the rest as a command. Quote only those, so the
    # scalars stay byte-identical for consumers that parse this file.
    shell = separator == "="
    lines = ["# Generated by scripts/layout.py -- do not edit."]
    for key, value in variables.items():
        if shell and " " in value:
            value = "'%s'" % value
        lines.append("SHADOWFAX_%s%s%s" % (key, separator, value))
    with open(path, "w") as out:
        out.write("\n".join(lines) + "\n")


def write_h(path, values, regions):
    lines = [
        "/* Generated by scripts/layout.py -- do not edit. */",
        "#ifndef SHADOWFAX_LAYOUT_H",
        "#define SHADOWFAX_LAYOUT_H",
        "",
    ]
    for key in sorted(values):
        lines.append("#define SHADOWFAX_%-26s %#xUL" % (key, values[key]))
    lines.append("")
    for name in sorted(regions):
        region = regions[name]
        lines.append("#define SHADOWFAX_REGION_%-24s %#xUL" % (ident(name) + "_BASE", region.base))
        lines.append("#define SHADOWFAX_REGION_%-24s %#xUL" % (ident(name) + "_SIZE", region.size))
    lines += ["", "#endif /* SHADOWFAX_LAYOUT_H */"]
    with open(path, "w") as out:
        out.write("\n".join(lines) + "\n")


def write_json(path, values, flags, regions, platform, filled):
    ram_base, ram_end = platform.ram
    data = {
        "memory": {"base": ram_base, "size": ram_end - ram_base},
        "qemu_mem": human(ram_end - ram_base),
        "values": {key.lower(): value for key, value in values.items()},
        "flags": {key.lower(): value for key, value in flags.items()},
        "regions": {
            name: {"base": r.base, "size": r.size, "mmio": r.mmio}
            for name, r in regions.items()
        },
        "reserved": [{"name": n, "base": b, "size": s} for n, b, s in platform.reserved],
        "domains": [
            {
                "name": d.name,
                "tsm": d.is_tsm,
                "boot": d.boot,
                "next_addr": d.next_addr,
                "regions": [r.name for r in d.regions],
                "permissions": {r.name: p for r, p in zip(d.regions, d.permissions)},
            }
            for d in platform.domains
        ],
        "artifacts": filled,
    }
    with open(path, "w") as out:
        out.write(json.dumps(data, indent=2) + "\n")


# --------------------------------------------------------------------------


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("dts", help="platform device tree source")
    parser.add_argument("out_dir", help="directory to write the blob and exports into")
    parser.add_argument(
        "--artifacts",
        default="target/artifacts",
        help="directory holding the staged TSM image and signature",
    )
    parser.add_argument("--include", action="append", default=[], help="dtc include path")
    args = parser.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)
    dtb_path = os.path.join(args.out_dir, "device-tree.dtb")

    command = ["dtc", "-q", "-I", "dts", "-O", "dtb"]
    for path in args.include:
        command += ["-i", path]
    with tempfile.NamedTemporaryFile(mode="r+", dir=args.out_dir) as depfile:
        result = subprocess.run(
            command + ["-d", depfile.name, args.dts], check=True, stdout=subprocess.PIPE
        )
        # dtc names stdout as its target; both build entry points need the same inputs.
        dependencies = depfile.read().split(":", 1)[1]
    fdt = libfdt.Fdt(bytearray(result.stdout))

    filled = fill_artifact_sizes(fdt, args.artifacts)
    fdt.pack()
    platform = Platform(fdt)
    found = problems(platform, filled)
    if found:
        print("%s: invalid memory map" % args.dts, file=sys.stderr)
        for problem in found:
            print("  error: %s" % problem, file=sys.stderr)
        return 1

    values, flags, regions = exports(platform)
    write_assignments(os.path.join(args.out_dir, "layout.mk"), values, flags, regions, platform, " := ")
    write_assignments(os.path.join(args.out_dir, "layout.sh"), values, flags, regions, platform, "=")
    write_h(os.path.join(args.out_dir, "layout.h"), values, regions)
    write_json(os.path.join(args.out_dir, "layout.json"), values, flags, regions, platform, filled)
    with open(os.path.join(args.out_dir, "layout.d"), "w") as depfile:
        depfile.write("%s %s:%s" % (dtb_path, os.path.join(args.out_dir, "layout.mk"), dependencies))
    with open(dtb_path, "wb") as blob:
        blob.write(fdt.as_bytearray())
    return 0


if __name__ == "__main__":
    sys.exit(main())
