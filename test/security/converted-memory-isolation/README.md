# Converted-memory isolation

This test checks that an untrusted supervisor domain loses access to the
memory it converts for a TVM.

The host converts the metadata and guest RAM, creates a TVM, and runs it. The
TVM fills a 4 KiB secret buffer with `0xa5` and shuts down. Without destroying
the TVM, the host installs a supervisor trap handler and reads the converted
guest RAM. The first load must raise a PMP load-access fault. If the loads
succeed, the test scans for the secret and reports where it was found.

Build and run with:

```sh
make -C test/security/converted-memory-isolation run
```

A successful run ends with:

```text
[ATTACK] load from 0x000000008c000000 raised an access fault
[ATTACK] PASS: converted memory is not accessible
```
