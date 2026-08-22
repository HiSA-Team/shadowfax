# Untrusted supervisor domain

This QEMU example runs three OpenSBI supervisor domains on two harts:

- domain 1 is Shadowfax's built-in TSM;
- domain 2 is the host on hart 0;
- domain 3 is an untrusted attacker on hart 1.

The test uses its own device-tree extension, `device-tree.dts.S`. It includes
the generic single-hart platform and adds CPU 1, the corresponding interrupt
bindings, and the attacker domain. The generic platform device tree is not
modified. The test runs the raw `bin/shadowfax.bin` firmware image with
QEMU's ordinary `-smp 2 -bios` boot flow: QEMU releases both harts at the
firmware reset entry, and OpenSBI starts the attacker supervisor domain on
hart 1.

The host creates and finalizes a 16 MiB TVM and then publishes its confidential
backing address through a 4 KiB coordination page shared only by the two
supervisor domains. This is ordinary OpenSBI domain memory; it is not mapped
into the TVM and does not require `COVH_ADD_TVM_SHARED_PAGES`.

The attacker attempts to load from the confidential address. Its supervisor
trap handler expects a PMP load-access fault. After the attacker reports that
the access was blocked, the host runs the TVM and checks that it still returns.

Build and run the scenario with:

```sh
make -C test/security/untrusted-supervisor-domain run
```
