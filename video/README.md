# Video
Here are 3 different demos:

- **helloworld**: Provides a complete CoVE demonstration using the multistep TVM creation flow
described in the paper. The hellotvm.c binary is mapped into guest memory at address 0x1000,
and a single vCPU is instantiated. The TVM prints a hello world message to the console.
Output is produced by forwarding an SBI ECALL from the TSM trap handler to the OpenSBI firmware.
- **hypervisor-standalone**: shows how to test the hypervisor without the multistep creation. This
is useful to test new guests and to collect measures which are not affected by the CoVE startup.
- **attestation**: this example shows a CoVE-G SBI call invoked by a guest to request platform
certificate evidence. Notably, we focus on the attestation mechanism rather than on cryptographic
verification, as this is intended for demonstration purposes only.
