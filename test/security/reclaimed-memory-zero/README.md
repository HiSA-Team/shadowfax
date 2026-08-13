# Reclaimed-memory zeroization test

This test models an untrusted supervisor attempting to read a TVM's RAM after
the TVM has shut down, been destroyed, and all donated memory has been
reclaimed.

The embedded test TVM writes `0xa5` into a 4 KiB secret buffer in guest RAM
and requests SBI SRST shutdown.  The host then destroys the TVM, reclaims its
metadata and guest RAM, and runs `malicious_function`.  That function reads
every byte of both reclaimed regions through a volatile pointer and fails on
the first nonzero byte.

Run it after building the firmware and generated inputs:

```sh
make -C test/security/reclaimed-memory-zero run
```

Expected final output:

```text
[ATTACK] metadata contains only zeroes
[ATTACK] guest RAM contains only zeroes
[ATTACK] PASS: no reclaimed TVM data is visible
```

The test intentionally reads 64 MiB after reclamation.  With the current
implementation reclaim also securely clears that RAM twice, once in the TSM
and once in Shadowfax, so QEMU can take a noticeable amount of time before
printing the attack result.
