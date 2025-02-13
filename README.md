# shadowfax

The codename `shadowfax project` aims to establish the foundation for an open-source software ecosystem for confidential computing on RISC-V, similar to ARM TrustFirmware.
The current RISC-V standard for confidential computing is defined in the RISC-V AP-TEE specification, also known as CoVE (**Co**nfidential **V**irtualization **E**xtension).

The CoVE specification outlines key details necessary for building Trusted Execution Environments (TEE) on RISC-V CPUs. While some aspects involve hardware components, CoVE is primarily a non-ISA specification, focusing on software deployment and execution models rather than hardware extensions. Notably, CoVE's most basic deployment model can run on existing CPUs that support the Hypervisor Extension. This model relies on a trusted hypervisor, referred to as the Trusted Security Monitor (TSM), to manage both untrusted virtual machines and confidential virtual machines, also known as TEE Virtual Machines (TVMs).

In the open-source ecosystem, only two CPUs currently support the H-extension: the CVA6 processor from the OpenHardware and PULP group, and the Rocket Core from Berkeley. Regarding TSM implementations, the only available open-source project supporting the CoVE specification is Salus, developed by Rivos. However, Salus remains a relatively simple implementation.

The most comprehensive CoVE deployment model is still a work in progress. This is largely due to its minimum hardware requirement: the Memory Protection Table (MPT) specified in the Smmpt extension, which is not yet stably supported by any CPU. There is, however, an open-source MPT IP implemented in SystemVerilog, with ongoing efforts to integrate it into the CVA6 CPU. The MPT enables the isolation of the TSM from the untrusted hypervisor, thereby reducing the Trusted Computing Base (TCB) and enforcing the principle of least privilege. A crucial requirement for this setup is that the TSM includes a TSM-Driver running in Machine mode alongside an SBI implementation, such as OpenSBI.

The codename `shadowfax project` has the following goals:
- Develop an open-source TSM-Driver that runs alongside OpenSBI.
- Implement the core functionalities of the CoVE SBI specification.
- Enable Supervisor Domain management using the MPT if available, or the PMP as a fallback.
- Write the implementation in a memory-safe language (e.g., Rust).

Additionally, the repository will serve as a reference point for the current state of compatible hardware and software technologies in the RISC-V confidential computing ecosystem.
