# TSM

This project is a `no_std` Rust implementation of a Trusted Security Monitor (TSM) for RISC-V, designed to support confidential computing via the CoVE (Confidential Virtual Machine Extensions) framework. It handles the lifecycle of Trusted Virtual Machines (TVMs), including memory isolation, attestation, and VCPU management.


## Key Features
- **SBI COVH Interface**: Implements Supervisor Binary Interface (SBI) calls for TVM management, such as `SBI_COVH_CREATE_TVM`, `SBI_COVH_CONVERT_PAGES`, and `SBI_COVH_RUN_TVM_VCPU`.
- **Memory Management**: Supports converting system memory into confidential memory pools for guest use.
- **Attestation**: integrates with a DICE-based attestation layer to provide secure identity for TVMs.
- **Performance Monitoring**: Includes built-in utilities to measure cycles, instructions, and time during the bootstrap process.

## Getting Started

### Project structure
- `src/main.rs`: The entry point, assembly boot code, and the primary SBI handler.
- `src/hyper.rs`: Contains the HypervisorState and logic for loading and managing TVMs.
- `src/state.rs`: Defines the TSM state, capabilities, and versioning.
- `src/h_extension`: Handles RISC-V H-extension specific features like CSRs and instruction emulation.

### Configuring the entrypoint
To run or test the TSM, you must manually configure the `_start` function in tsm/src/main.rs to point to your desired execution path.

```rust
// src/main.rs

#[no_mangle]
#[unsafe(naked)]
extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        // ... (stack setup)
        call {main} // Change this symbol to the desired function
        ",
        stack_size_per_hart = const STACK_SIZE_PER_HART,
        stack_top = sym _stack_top,
        main = sym test_tvm_bootstrap, // Options: main, test_tvm_bootstrap, test_tvm_bootstrap_perf
    )
}
```

### Available entrypoints

1. `main`: The standard production entry point that waits for SBI calls from a host.
2. `test_tvm_bootstrap`: A test runner that manually initializes the TSM, converts memory, loads a static guest ELF (defined in GUEST_ELF), and immediately executes it.
3. `test_tvm_bootstrap_perf`: A performance-oriented version of the bootstrap test that prints the cycles, instructions, and time taken to initialize and enter the guest.

## ELF Loading Approaches

When using the test functions, you can modify the loading strategy within `test_tvm_bootstrap` or `test_tvm_bootstrap_perf`.
You can choose between a static or dynamic approach by swapping the helper function called from the hyper module:

-  Dynamic (Lazy) Approach: `Use bootstrap_load_elf_lazy`. This maps guest memory regions but may defer actual page loading.
- Static Approach: Use `bootstrap_load_elf`. This performs a full pre-loading of the ELF segments into the TVM's confidential memory.

## Run a bare-metal guest

The freestanding VS-mode workloads live in `guests/bare-metal/`. Build them from the repository
root, select the resulting ELF in `tsm/src/main.rs`, build the TSM, and run it directly with QEMU.

```sh
make -C guests bare-metal
```

```rust
// Point GUEST_ELF to the selected bare-metal guest.
#[link_section = ".rodata"]
pub static GUEST_ELF: [u8;
    include_bytes!("../../guests/bare-metal/hellotvm.out").len()
] = *include_bytes!("../../guests/bare-metal/hellotvm.out");
```

```sh
make -B tsm
qemu-system-riscv64 -M virt -nographic -smp 1 -m 1G \
    -kernel target/riscv64imac-unknown-none-elf/debug/tsm
```

## Run the Linux TVM guest

Linux running as a confidential TVM guest is separate from Linux running as Shadowfax's untrusted
host. The standalone TSM currently embeds these generated inputs:

```text
linux/guest/vmlinux
bin/linux-tvm.dtb
bin/linux-tvm-initramfs.cpio.gz
```

The reproducible source inputs are committed under `guests/linux/`:

```text
guests/linux/kernel.config
guests/linux/busybox.config
guests/linux/linux-tvm.dts
```

See [`../guests/linux/README.md`](../guests/linux/README.md) for the complete build, DTB patching,
and boot procedure. The current test entrypoint uses the lazy ELF loader, normalizes the kernel's ELF
physical addresses into a 256 MiB guest-physical RAM region starting at `0x00200000`, and supplies the
DTB address in the Linux boot argument register.
