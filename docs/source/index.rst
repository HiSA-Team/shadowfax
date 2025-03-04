.. shadowfax documentation master file, created by
   sphinx-quickstart on Tue Mar  4 13:47:57 2025.
   You can adapt this file completely to your liking, but it should at least
   contain the root `toctree` directive.

shadowfax documentation
=======================

The codename **shadowfax project** aims to establish the foundation for an
open-source software ecosystem for confidential computing on RISC-V, similar
to ARM TrustFirmware. The current RISC-V standard for confidential computing
is defined in the RISC-V AP-TEE specification, also known as CoVE 
(**Co** nfidential **V** irtualization **E** xtension).

The codename **shadowfax project** has the following goals:

- Develop an open-source TSM-Driver that runs alongside OpenSBI.
- Implement the core functionalities of the CoVE SBI specification.
- Enable Supervisor Domain management using the MPT if available, or the PMP as a fallback.
- Write the implementation in a memory-safe language (e.g., Rust).

.. note::

   This project is under active development.

.. toctree::
   :maxdepth: 2
   :caption: Contents:

