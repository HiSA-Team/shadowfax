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

use super::{SBI_EXT_SUPD_GET_ACTIVE_DOMAINS, SUPD_EXT_ID, SUPD_EXT_NAME};
use crate::{opensbi, sbi::SbiRet, shadowfax_core::state::STATE};

#[link_section = ".data.supd_ext"]
pub static mut SBI_SUPD_EXTENSION: opensbi::sbi_ecall_extension = opensbi::sbi_ecall_extension {
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

/// SBI ecall handler for the SUPD extension.
///
/// All ecalls targeting the SUPD extension ID are routed here. The `fid` (function ID)
/// determines which SUPD operation to perform.
///
/// Parameters:
/// - _extid: the SBI extension ID (should equal SUPD_EXT_ID)
/// - fid:    the function identifier within this extension
/// - regs:   pointer to the trap registers (holds arguments in a0–a7)
/// - ret:    pointer to the SBI return struct (used to convey return values)
///
/// Returns:
/// - SBI_SUCCESS (0) on success, setting `ret.value` appropriately.
/// - SBI_ENOTSUPP if the `fid` is not recognized.
pub unsafe extern "C" fn sbi_supd_handler(
    _extid: u64,
    fid: u64,
    _regs: *mut opensbi::sbi_trap_regs,
    ret: *mut opensbi::sbi_ecall_return,
) -> i32 {
    match fid {
        SBI_EXT_SUPD_GET_ACTIVE_DOMAINS => {
            // SUPD_FID_GET_ACTIVE_DOMAINS
            debug!("sbi_supd_get_active_domains()");
            let result = sbi_supd_get_active_domains();
            (*ret).value = result.value as u64;

            result.error as i32
        }
        _ => {
            // Unsupported function ID
            opensbi::sbi_printf("unsupported supd fid\n\0".as_ptr());
            opensbi::SBI_ENOTSUPP
        }
    }
}

/// SUPD operation: get the set of currently active domains.
///
/// Parameters:
///
/// Returns:
/// - Sbiret.error =
/// - Sbiret.value = a bitmask of active‐domain identifiers.
fn sbi_supd_get_active_domains() -> SbiRet {
    let mut ret: isize = 0;
    for (i, _) in STATE.lock().get().unwrap().get_domains().iter().enumerate() {
        ret |= 1 << i;
    }

    SbiRet {
        error: 0,
        value: ret,
    }
}
