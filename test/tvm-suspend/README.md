# TVM suspend/resume test

This standalone test embeds a small RISC-V guest directly in the test build.
The guest prints `TVM startup`, requests SBI HSM hart suspend, then prints
`TVM resume` after it is entered again.

The untrusted launcher calls `SBI_COVH_RUN_TVM_VCPU` twice:

1. The first call enters the guest and returns when the guest suspends.
2. The second call resumes the saved vCPU context.

After the guest shuts down, the launcher destroys the TVM, reclaims its
memory, and prints a pass message.

Build and run it after building the firmware inputs:

```sh
make -C test/tvm-suspend all
make -C test/tvm-suspend run
```
