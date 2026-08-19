# Malicious TVM isolation test

This self-contained test creates a trusted TVM and a malicious TVM from the
same guest ELF. The trusted guest writes a secret, suspends, and later checks
that the secret is intact. The malicious launcher first attempts to alias the
trusted physical page into the malicious TVM and must be rejected. The
malicious guest then scans the same GPA and must not observe the secret.

Run after building the CoVE firmware inputs:

```sh
make -C test/security/malicious-tvm all
make -C test/security/malicious-tvm run
```

Expected attack result:

```text
[ATTACK] physical alias rejected
[MALICIOUS] target GPA is not readable
[TRUSTED] secret intact
[HOST] PASS: malicious TVM could not access trusted data
```
