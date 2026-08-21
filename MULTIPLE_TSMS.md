# Running multiple TSMs

Shadowfax can load a separate trusted security monitor (TSM) into each trusted
OpenSBI supervisor domain. Each TSM instance owns its domain memory, heap, and
global state, so one untrusted host can create TVMs through multiple isolated
TSMs.

The example in `test/multiple-supervisor-domains/` creates this layout:

| Domain ID | Role | TSM source | Address range |
| --- | --- | --- | --- |
| 1 | Trusted | Built into Shadowfax | `0x90000000-0x92000000` |
| 2 | Untrusted boot domain | None | `0x8a000000-0x90000000` |
| 3 | Trusted | Raw-staged by QEMU | `0x92000000-0x94000000` |

OpenSBI assigns supervisor-domain IDs in device-tree order, with domain zero
reserved for the root domain. CoVE-H function IDs encode the destination domain
in bits 31:26. The example uses `covh_call_to()` to create one TVM through
domain 1 and another through domain 3.

## Run the example

Initialize submodules and generate the development signing and DICE keys once:

```sh
git submodule update --init --recursive
make PYTHON='uv run --with cbor2' generate-keys
```

Build the position-independent TSM, firmware, signatures, DICE input, and
platform artifacts, then run the custom launcher:

```sh
make -B PYTHON='uv run --with cbor2' PLATFORM=generic firmware
make -C test/multiple-supervisor-domains all
make -C test/multiple-supervisor-domains run
```

The launcher generates its custom DTB and supplies QEMU loader devices for the
external TSM ELF and signature. A successful run ends with:

```text
[TVM] Hello world from trusted domain 1
[HOST] TVM 1 returned
[TVM] Hello world from trusted domain 3
[HOST] TVM 2 returned
[HOST] PASS: multi-supervisor-domain TVMs completed
```

By default, both domains run the project TSM: domain 1 uses the copy embedded
in Shadowfax, while QEMU stages another copy for domain 3. Test another TSM
implementation by overriding both files:

```sh
make -C test/multiple-supervisor-domains run \
    EXTERNAL_TSM=/absolute/path/to/tsm.elf \
    EXTERNAL_TSM_SIGNATURE=/absolute/path/to/tsm.elf.signature
```

The external image must be signed by the private key corresponding to
`shadowfax/keys/publickey.pem`. For a development image, create the signature
with:

```sh
openssl pkeyutl -sign \
    -inkey shadowfax/keys/privatekey.pem \
    -in /absolute/path/to/tsm.elf \
    -out /absolute/path/to/tsm.elf.signature
```

## Select the TSM source in the device tree

Mark every domain that runs a TSM with `shadowfax,tsm`. Select the project TSM
embedded in the firmware with `shadowfax,load-tsm`:

```dts
trusted-domain {
    compatible = "opensbi,domain,instance";
    regions = <&trusted_memory 0x3f>, <&trusted_devices 0x3f>;
    next-addr = <0x0 0x90000000>;
    next-mode = <0x1>;
    shadowfax,tsm;
    shadowfax,load-tsm;
};
```

For an externally supplied TSM, omit `shadowfax,load-tsm` and describe the
physical staging locations of the ELF and its 64-byte Ed25519 signature:

```dts
/ {
    reserved-memory {
        #address-cells = <2>;
        #size-cells = <2>;
        ranges;

        tsm-images@94000000 {
            reg = <0x0 0x94000000 0x0 0x02000000>;
            no-map;
        };
    };
};

trusted-domain-secondary {
    compatible = "opensbi,domain,instance";
    regions = <&trusted_memory_secondary 0x3f>, <&trusted_devices 0x3f>;
    next-addr = <0x0 0x92000000>;
    next-mode = <0x1>;
    shadowfax,tsm;
    shadowfax,tsm-image = <0x0 0x94000000 0x0 IMAGE_SIZE>;
    shadowfax,tsm-signature = <0x0 0x95fff000 0x0 0x40>;
};
```

Every external staging range must be fully contained in a `no-map`
`/reserved-memory` region and must not overlap any supervisor-domain memory.
The image and signature properties must be supplied together. Selecting both
an external image and `shadowfax,load-tsm` is rejected.

QEMU only copies the external files into the declared staging addresses. It
does not parse or relocate the TSM:

```sh
-device loader,file=/path/to/tsm.elf,addr=0x94000000,force-raw=on
-device loader,file=/path/to/tsm.elf.signature,addr=0x95fff000,force-raw=on
```

Shadowfax verifies the signature before parsing the ELF, measures the signed
ELF bytes with SHA-512, derives the TSM DICE layer from that measurement, and
then loads the image into its supervisor domain.

## Relocatable TSM image

The project TSM is built once as a position-independent RISC-V `ET_DYN` ELF and
can therefore be instantiated at the start of more than one trusted domain.
Its link address and ELF entry are zero. At boot, Shadowfax calculates the load
bias from the domain's `next-addr`, places every `PT_LOAD` segment at that bias,
zeros the segment BSS, and applies `.rela.dyn` entries.

The loader deliberately accepts only `R_RISCV_RELATIVE` relocations with symbol
index zero. Relocation targets must fall inside a writable loaded segment, and
the ELF entry and `_secure_init` must fall inside executable loaded segments.
All loaded ranges must fit inside non-MMIO memory assigned to the destination
domain with the requested ELF permissions.

The project build uses:

```text
-C relocation-model=pic -C link-arg=-pie
```

It also rebuilds `core` and `alloc` with `-Z build-std=core,alloc`, because a
PIE cannot safely link against a prebuilt non-PIC bare-metal sysroot. The TSM
linker script uses `ORIGIN = 0`; its `_start` code derives the runtime base from
the program counter before setting up the per-domain stack. Absolute linker
addresses must not be used for runtime state unless they are represented by a
supported dynamic relocation.

A fixed-address RISC-V `ET_EXEC` TSM remains supported when its ELF entry
already equals the domain's `next-addr`, but it cannot be reused at different
domain start addresses. New external implementations should use the relocatable
`ET_DYN` format.

## Secure initialization ABI

An external ELF must retain `_secure_init` in its static symbol table and place
it in an executable load segment. Shadowfax calls it after authentication,
measurement, loading, and relocation:

```c
intptr_t _secure_init(uintptr_t boot_info_addr);
```

The versioned C structure is defined in
[`common/include/shadowfax_tsm.h`](common/include/shadowfax_tsm.h). It supplies:

- the ABI magic, version, and structure size;
- the destination supervisor-domain ID;
- the ELF load base;
- the SHA-512 image measurement;
- the physical address and size of the serialized DICE context.

The DICE context contains three length-prefixed byte strings: a little-endian
`u32` CDI length and CDI, a little-endian `u32` platform-token length and CBOR
token, then a little-endian `u32` TSM-token length and CBOR token.

`_secure_init` must validate the versioned record, copy any referenced data it
needs before returning, initialize its private state, and return zero. A
nonzero result aborts Shadowfax domain initialization.
