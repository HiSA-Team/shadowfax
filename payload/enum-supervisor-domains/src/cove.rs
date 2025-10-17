/*
 * This file contains all public types used by both TSM and the VMM.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

/*
 * TsmInfo is a data structure that holds information about the Trusted Software Module (TSM).
 * It includes the current state of the TSM, its implementation identifier, version, supported
 * capabilities, and memory requirements for managing Trusted Virtual Machines (TVMs).
 */
#[repr(C, align(4))]
#[derive(Clone, Debug)]
pub struct TsmInfo {
    /*
     * The current state of the TSM (see `tsm_state` enum above).
     * If the state is not `TSM_READY`, the remaining fields are invalid and
     * will be initialized to `0`.
     */
    pub tsm_state: TsmState,
    /*
     * Identifier of the TSM implementation, see `Reserved TSM Implementation IDs`
     * table below. This identifier is intended to distinguish among different TSM
     * implementations, potentially managed by different organizations, that might
     * target different deployment models and, thus, implement subset of CoVE spec.
     */
    pub tsm_impl_id: u32,
    /*
     * Version number of the running TSM.
     */
    pub tsm_version: u32,
    /*
     * A bitmask of CoVE features supported by the running TSM, see `TSM Capabilities`
     * table below. Every bit in this field corresponds to a capability defined by
     * `COVE_TSM_CAP_*` constants. Presence of bit `i` indicates that both the TSM
     * and hardware support the corresponding capability.
     */
    pub tsm_capabilities: usize,
    /*
     * The number of 4KB pages which must be donated to the TSM for storing TVM
     * state in sbi_covh_create_tvm_vcpu(). `0` if the TSM does not support the
     * dynamic memory allocation capability.
     */
    pub tvm_state_pages: usize,
    /*
     * The maximum number of vCPUs a TVM can support.
     */
    pub tvm_max_vcpus: usize,
    /*
     * The number of 4KB pages which must be donated to the TSM when creating
     * a new vCPU. `0` if the TSM does not support the dynamic memory allocation
     * capability.
     */
    pub tvm_vcpu_state_pages: usize,
}
/*
 * Sbiret is a structure used to return the result of an SBI (Supervisor Binary Interface) call.
 * It contains an error code and a value, which provide information about the success or failure
 * of the call and any resulting data.
 */
#[repr(C)]
pub struct SbiRet {
    pub error: isize,
    pub value: isize,
}

/*
 * TsmPageType is an enumeration that defines the types of memory pages supported by the TSM.
 * It includes options for 4 KiB, 2 MiB, 1 GiB, and 512 GiB pages, allowing for flexible memory
 * management and allocation.
 */
pub enum TsmPageType {
    /* 4 KiB */
    Page4k = 0,
    /* 2 MiB */
    Page2mb = 1,
    /* 1 GiB */
    Page1gb = 2,
    /* 512 GiB */
    Page512gb = 3,
}

/*
 * TvmState is an enumeration that represents the state of a Trusted Virtual Machine (TVM).
 * It indicates whether the TVM is in the process of initialization or is ready to run.
 */
#[derive(Clone)]
pub enum TvmState {
    /* The TVM has been created, but isn't yet ready to run */
    TvmInitializing = 0,
    /* The TVM is in a runnable state */
    TvmRunnable = 1,
}

/*
 * TsmState is an enumeration that describes the current state of the Trusted Software Module (TSM).
 * It provides information on whether the TSM is not loaded, loaded but not initialized, or fully
 * initialized and ready to accept ECALLs (environment calls).
 */
#[derive(Clone, Debug)]
pub enum TsmState {
    /* TSM has not been loaded on this platform. */
    TsmNotLoaded = 0,
    /* TSM has been loaded, but has not yet been initialized. */
    TsmLoaded = 1,
    /* TSM has been loaded & initialized, and is ready to accept ECALLs.*/
    TsmReady = 2,
}
