# Multiple supervisor domains

This standalone example boots one untrusted supervisor domain and two trusted
supervisor domains. The first domain uses Shadowfax's built-in relocatable TSM;
QEMU stages the second domain's signed ELF for Shadowfax to verify, measure,
relocate, and load. Each resulting TSM has its own heap and global state.

The generated tree retains trusted and untrusted domain IDs 1 and 2; the
appended externally supplied trusted domain receives ID 3. Override
`EXTERNAL_TSM` and `EXTERNAL_TSM_SIGNATURE` to test another implementation
that follows the Shadowfax TSM ELF and secure-init ABI. See
[Running multiple TSMs](../../MULTIPLE_TSMS.md) for the device-tree contract,
relocation model, signing requirements, and external TSM ABI.

Build the platform artifacts and run the example:

```sh
make -C ../.. firmware
make all
make run
```

The relevant output ends with:

```text
[TVM] Hello world from trusted domain 1
[HOST] TVM 1 returned
[TVM] Hello world from trusted domain 3
[HOST] TVM 2 returned
[HOST] PASS: multi-supervisor-domain TVMs completed
```
