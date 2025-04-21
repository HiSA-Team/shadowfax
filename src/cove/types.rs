#[repr(C)]
#[derive(Clone)]
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

#[repr(C)]
pub struct Sbiret {
    pub error: usize,
    pub value: usize,
}

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

#[derive(Clone)]
pub enum TvmState {
    /* The TVM has been created, but isn't yet ready to run */
    TvmInitializing = 0,
    /* The TVM is in a runnable state */
    TvmRunnable = 1,
}

#[derive(Clone)]
pub enum TsmState {
    /* TSM has not been loaded on this platform. */
    TsmNotLoaded = 0,
    /* TSM has been loaded, but has not yet been initialized. */
    TsmLoaded = 1,
    /* TSM has been loaded & initialized, and is ready to accept ECALLs.*/
    TsmReady = 2,
}
