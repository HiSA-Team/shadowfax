/*
 * This is where the main cove implementation lies. This module exposes the `init()` function
 * that register the coveh sbi extension and initializes the state. The state is represented
 * by the static variable `SHADOWFAX_INFO` which is protected by a SpinMutex.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

use spin::mutex::SpinMutex;

use crate::opensbi;

use super::{
    Sbiret, TsmInfo, TsmState, COVEH_EXT_ID, COVEH_EXT_NAME, SBI_EXT_COVE_HOST_CONVERT_PAGES,
    SBI_EXT_COVE_HOST_CREATE_TVM, SBI_EXT_COVE_HOST_GET_TSM_INFO, SHADOWFAX_IMPL_ID,
};

/*
 * This static variable represents the global state of the TSM (Trusted Software Module).
 * It is protected by a SpinMutex to ensure safe concurrent access across different threads.
 * The TsmInfo struct holds various state information about the TSM, such as its current state,
 * implementation ID, version, capabilities, and other info.
 *
 */
#[link_section = ".data"]
static SHADOWFAX_INFO: SpinMutex<TsmInfo> = SpinMutex::new(TsmInfo {
    tsm_state: TsmState::TsmNotLoaded,
    tsm_impl_id: SHADOWFAX_IMPL_ID,
    tsm_version: 0,
    tsm_capabilities: 0,
    tvm_state_pages: 0,
    tvm_max_vcpus: 0,
    tvm_vcpu_state_pages: 0,
});

/*
 * The coveh handler as mandated by Opensbi. Each ecall targeting this extension is
 * routed to this function. Based on fid (function id) and according to the CoVE
 * specification all required function will be implmented here.
 *
 */
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
        SBI_EXT_COVE_HOST_GET_TSM_INFO => {
            let result = sbi_covh_get_tsm_info(regs.a0, regs.a1);
            ret.value = result.value as u64;

            opensbi::SBI_SUCCESS as i32
        }
        SBI_EXT_COVE_HOST_CONVERT_PAGES => {
            todo!()
        }
        SBI_EXT_COVE_HOST_CREATE_TVM => {
            todo!()
        }
        // Default case for unsupported function IDs, logs a message and returns an error.
        _ => {
            opensbi::sbi_printf("unsupported fid\n\0".as_ptr());
            opensbi::SBI_ENOTSUPP
        }
    }
}

/*
 * This function initialize the coveh extension by registering an opensbi extension
 * and set the SHADOWFAX_INFO.TsmState to TsmState::TsmReady.
 *
 */
#[link_section = ".text"]
pub fn init() {
    // First we need to register the cove host extension using the OpenSBI API.
    // The goal is to register an handler (sbi_coveh_handler) when our extension
    // is called with an ecall.
    let mut extension = opensbi::sbi_ecall_extension {
        experimental: true,
        probe: None,
        name: COVEH_EXT_NAME,
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

    // This section should make validation checks for the TSM-driver and
    // init the global state.
    let mut info = SHADOWFAX_INFO.lock();
    info.tsm_state = TsmState::TsmLoaded;
    // TODO: make the actual check to understand what platform do we have
    // what capabilities do we have, perform integrity check and validate the
    // TCB.

    info.tsm_capabilities = 0;
    info.tsm_state = TsmState::TsmReady;
}

/*
 * Retrieves the current TSM state, configuration, and supported features.
 *
 * Parameters:
 * - tsm_info_address: A 4-byte aligned physical memory address where the TSM will write the TsmInfo struct.
 * - tsm_info_len: The size of the TsmInfo struct.
 *
 * Returns:
 * - The number of bytes written to tsm_info_address on success.
 */
fn sbi_covh_get_tsm_info(tsm_info_address: u64, tsm_info_len: u64) -> Sbiret {
    let needed = core::mem::size_of::<TsmInfo>() as u64;

    // TODO: check if the address is valid
    if tsm_info_len < needed {
        return Sbiret {
            error: opensbi::SBI_ERR_INVALID_PARAM as usize,
            value: 0,
        };
    }

    unsafe {
        let info = SHADOWFAX_INFO.lock();
        let tsm_info_ptr = tsm_info_address as *mut TsmInfo;
        tsm_info_ptr.write(info.clone())
    }
    Sbiret {
        error: 0,
        value: needed as usize,
    }
}

fn sbi_covh_convert_pages(base_page_address: u64, num_pages: u64) -> Sbiret {
    todo!()
}
