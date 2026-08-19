# Bare-metal guest support

This directory contains the common runtime and build support used by Shadowfax
bare-metal payloads and test launchers.

`config.mk` exports the paths and flags needed by consumers:

- `GUEST_CFLAGS` adds the public headers in `include/`.
- `GUEST_ASFLAGS` locates shared assembly sources.
- `GUEST_LIB` names the static `libbaremetal.a` archive and can be used as an
  explicit Make dependency.
- `GUEST_LDFLAGS` links that archive after the consumer's object files.

The library provides SBI and CoVE calls, console output, freestanding memory
helpers, generic alignment helpers for non-zero power-of-two alignments, and
the configurable ELF payload loader. `vmstartup.S` and `linker.ld` form the
payload runtime.

Test launchers set their scenario-specific variables and include `launcher.mk`:

```make
ROOT          := ../..
LAUNCHER_NAME := example
PAYLOAD_SRC   := guest.c

include $(ROOT)/guests/bare-metal/launcher.mk
```

The fragment also supports an externally built `GUEST_ELF`, an optional
`GUEST_DTB`, custom metadata size, split or contiguous guest memory, and the
existing firmware, DTB, QEMU, build-directory, and load-address overrides.
`GUEST_MEMORY_SIZE` selects the per-TVM guest RAM size in bytes and defaults to
4 MiB. Tests hosting multiple TVMs set `GUEST_MEMORY_COUNT` so the linker
reserves the total, while the latency test explicitly retains 256 MiB. The
requested total must fit within the guest region described by the DTB. The
size is part of a build stamp, so changing it rebuilds the affected launcher
without requiring `make clean`.
