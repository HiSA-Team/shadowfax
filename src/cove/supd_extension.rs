/*
 * SUPD SBI Extension Module
 *
 * This module implements the SUPD (Supervisor‐Domain) SBI extension. It exposes two
 * entry points:
 *  1. `init()` — registers the SUPD extension with OpenSBI.
 *  2. `sbi_supd_handler()` — the ecall handler that dispatches SUPD function IDs.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

use super::{
    cove_host_extension::TSM_INFO, SbiRet, TsmState, SBI_EXT_SUPD_GET_ACTIVE_DOMAINS, SUPD_EXT_ID,
    SUPD_EXT_NAME,
};
use crate::opensbi;

static mut SBI_SUPD_EXTENSION: opensbi::sbi_ecall_extension = opensbi::sbi_ecall_extension {
    experimental: true,
    probe: None,
    name: SUPD_EXT_NAME,
    extid_start: SUPD_EXT_ID,
    extid_end: SUPD_EXT_ID,
    handle: Some(sbi_supd_handler),
    register_extensions: None,
    head: opensbi::sbi_dlist {
        next: core::ptr::null_mut(),
        prev: core::ptr::null_mut(),
    },
};

/*
 * SBI ecall handler for the SUPD extension.
 *
 * All ecalls targeting the SUPD extension ID are routed here. The `fid` (function ID)
 * determines which SUPD operation to perform.
 *
 * Parameters:
 * - _extid: the SBI extension ID (should equal SUPD_EXT_ID)
 * - fid:    the function identifier within this extension
 * - regs:   pointer to the trap registers (holds arguments in a0–a7)
 * - ret:    pointer to the SBI return struct (used to convey return values)
 *
 * Returns:
 * - SBI_SUCCESS (0) on success, setting `ret.value` appropriately.
 * - SBI_ENOTSUPP if the `fid` is not recognized.
 */
#[link_section = ".text"]
pub unsafe extern "C" fn sbi_supd_handler(
    _extid: u64,
    fid: u64,
    _regs: *mut opensbi::sbi_trap_regs,
    ret: *mut opensbi::sbi_ecall_return,
) -> i32 {
    match fid {
        SBI_EXT_SUPD_GET_ACTIVE_DOMAINS => {
            // SUPD_FID_GET_ACTIVE_DOMAINS
            opensbi::sbi_printf("called sbi_supd_get_active_domains\n\0".as_ptr());
            let result = sbi_supd_get_active_domains();
            (*ret).value = result.value as u64;

            opensbi::sbi_printf("returned from sbi_supd_get_active_domains\n\0".as_ptr());

            result.error as i32
        }
        _ => {
            // Unsupported function ID
            opensbi::sbi_printf("unsupported supd fid\n\0".as_ptr());
            opensbi::SBI_ENOTSUPP
        }
    }
}

/*
 * Initialize and register the SUPD extension with OpenSBI.
 *
 * This must be called during SBI bring‐up to make the SUPD ecall available.
 * It constructs an `sbi_ecall_extension` record and registers it.
 */
#[link_section = ".text"]
pub fn init() -> i32 {
    // SAFETY: we trust OpenSBI to correctly link this extension into its ecall handlers
    unsafe { opensbi::sbi_ecall_register_extension(&raw mut SBI_SUPD_EXTENSION) }
}

/*
 * SUPD operation: get the set of currently active domains.
 *
 * Parameters:
 * - active_domains: a bitmask or pointer implementation writes out active‐domain
 * identifiers.
 *
 * Returns:
 * - Sbiret.error = 0 on success (always 0 in this stub).
 * - Sbiret.value = number of domains written (0 in this stub).
 */
fn sbi_supd_get_active_domains() -> SbiRet {
    let mut ret: isize = 0;
    for (i, tsm) in TSM_INFO.lock().iter().enumerate() {
        if matches!(tsm.tsm_state, TsmState::TsmReady) {
            ret |= 1 << i;
        }
    }

    SbiRet {
        error: 0,
        value: ret,
    }
}
