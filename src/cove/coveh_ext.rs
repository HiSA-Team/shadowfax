use spin::mutex::SpinMutex;

use crate::opensbi;

use super::COVEH_EXT_ID;

const COVH_EXT_NAME: [u8; 8] = *b"covh\0\0\0\0";
const SHADOWFAX_IMPL_ID: u32 = 3;

struct Sbiret {
    error: usize,
    value: usize,
}

enum TsmPageType {
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
enum TvmState {
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

const COVE_TSM_CAP_PROMOTE_TVM: usize = 0x0;
const COVE_TSM_CAP_ATTESTATION_LOCAL: usize = 0x1;
const COVE_TSM_CAP_ATTESTATION_REMOTE: usize = 0x2;
const COVE_TSM_CAP_AIA: usize = 0x3;
const COVE_TSM_CAP_MRIF: usize = 0x4;
const COVE_TSM_CAP_MEMORY_ALLOCATION: usize = 0x5;

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

#[link_section = ".data"]
static SHADOWFAX_INFO: SpinMutex<TsmInfo> = SpinMutex::new(TsmInfo {
    tsm_state: TsmState::TsmNotLoaded,
    tsm_impl_id: SHADOWFAX_IMPL_ID,
    tsm_version: 0,
    tsm_capabilities: 0,
    tvm_state_pages: 0,
    tvm_max_vcpus: 1,
    tvm_vcpu_state_pages: 0,
});

#[link_section = ".text"]
pub unsafe extern "C" fn sbi_coveh_handler(
    _extid: u64,
    fid: u64,
    regs: *mut opensbi::sbi_trap_regs,
    ret: *mut opensbi::sbi_ecall_return,
) -> i32 {
    let regs = *regs;
    let mut ret = *ret;
    match fid {
        0 => {
            opensbi::sbi_printf("called sbi_covh_get_tsm_info\n\0".as_ptr());
            let result = sbi_covh_get_tsm_info(regs.a0, regs.a1);
            ret.value = result.value as u64;
            opensbi::sbi_printf("returned from sbi_covh_get_tsm_info\n\0".as_ptr());

            opensbi::SBI_SUCCESS as i32
        }
        _ => {
            opensbi::sbi_printf("unsupported fid\n\0".as_ptr());
            opensbi::SBI_ENOTSUPP
        }
    }
}

#[link_section = ".text"]
pub fn init() {
    let mut info = SHADOWFAX_INFO.lock();
    info.tsm_state = TsmState::TsmLoaded;

    let mut extension = opensbi::sbi_ecall_extension {
        experimental: true,
        probe: None,
        name: COVH_EXT_NAME,
        extid_start: COVEH_EXT_ID,
        extid_end: COVEH_EXT_ID,
        handle: Some(sbi_coveh_handler),
        register_extensions: None,
        head: opensbi::sbi_dlist {
            next: core::ptr::null_mut(),
            prev: core::ptr::null_mut(),
        },
    };

    unsafe { opensbi::sbi_ecall_register_extension(&mut extension) };

    info.tsm_capabilities = 0;
    info.tsm_state = TsmState::TsmReady;
}

fn sbi_covh_get_tsm_info(tsm_info_address: u64, tsm_info_len: u64) -> Sbiret {
    let needed = core::mem::size_of::<TsmInfo>() as u64;

    // TODO: check if an address is valid
    if tsm_info_len < needed {
        return Sbiret {
            error: opensbi::SBI_ERR_INVALID_PARAM as usize,
            value: 0,
        };
    }

    unsafe {
        let info = SHADOWFAX_INFO.lock().clone();
        let tsm_info_ptr = tsm_info_address as *mut TsmInfo;
        (*tsm_info_ptr).tsm_impl_id = info.tsm_impl_id;
    }
    Sbiret {
        error: 0,
        value: needed as usize,
    }
}
