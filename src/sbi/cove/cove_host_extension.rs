<<<<<<< HEAD
=======
/*
 * This is where the main cove implementation lies. This module exposes the `init()` function
 * that register the coveh sbi extension and initializes the state. The state is represented
 * by the static variable `SHADOWFAX_INFO` which is protected by a SpinMutex.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

use fdt_rs::{base::DevTree, prelude::FallibleIterator};
use heapless::Vec;
use spin::mutex::Mutex;

use crate::opensbi;

use super::{
    types::{SbiRet, TsmInfo, TsmState},
    COVEH_EXT_ID, COVEH_EXT_NAME, SBI_EXT_COVE_HOST_GET_TSM_INFO, SBI_EXT_COVE_HOST_PROMOTE_TO_TVM,
};

macro_rules! cove_unpack_fid {
    ($fid:expr) => {
        (($fid >> 26) & 0x3F, $fid & 0xFFFF)
    };
}

#[link_section = ".data.cove_ext"]
static mut SBI_COVE_HOST_EXTENSION: opensbi::sbi_ecall_extension = opensbi::sbi_ecall_extension {
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

/// This static variable represents the global state of the TSM (Trusted Software Module).
/// It is protected by a SpinMutex to ensure safe concurrent access across different threads.
/// The TsmInfo struct holds various state information about the TSM, such as its current state,
/// implementation ID, version, capabilities, and other info.
///
/// TODO: make this heap allocated with a static vector
#[link_section = ".data"]
pub static TSM_INFO: Mutex<Vec<TsmInfo, 64>> = Mutex::new(Vec::new());

/// The coveh handler as mandated by Opensbi. Each ecall targeting this extension is
/// routed to this function. Based on fid (function id) and according to the CoVE
/// specification all required function will be implmented here.
#[link_section = ".text"]
pub unsafe extern "C" fn sbi_coveh_handler(
    _extid: u64,
    fid: u64,
    regs: *mut opensbi::sbi_trap_regs,
    ret: *mut opensbi::sbi_ecall_return,
) -> i32 {
    let regs = *regs;
    let mut ret = *ret;
    let (sdid, fid) = cove_unpack_fid!(fid);
    match fid {
        SBI_EXT_COVE_HOST_GET_TSM_INFO => {
            debug!(
                "sbi_covh_get_tsm_info(sdid={}, addr=0x{:x}, size={})",
                sdid, regs.a0, regs.a1,
            );
            let result = sbi_covh_get_tsm_info(sdid as usize, regs.a0 as usize, regs.a1 as usize);
            ret.value = result.value as u64;

            result.error as i32
        }
        SBI_EXT_COVE_HOST_PROMOTE_TO_TVM => {
            debug!(
                "sbi_covh_promote_to_tvm(sdid={}, fdt_addr={:x}, tap_addr={:x}, entry_sepc=0x{:x}, tvm_identity_addr={:x})",
                sdid,
                regs.a0,
                regs.a1,
                regs.a2,
                regs.a3,
            );

            assert_eq!(sdid, 1, "Confidential domain must have id = 1");
            let result = sbi_covh_promote_to_tvm(
                regs.a0 as usize,
                regs.a1 as usize,
                regs.a2 as usize,
                regs.a3 as usize,
            );
            ret.value = result.value as u64;

            result.error as i32
        }
        // Default case for unsupported function IDs, logs a message and returns an error.
        _ => {
            debug!("unsupported fid: {}", fid);
            opensbi::SBI_ENOTSUPP
        }
    }
}

/// This function initialize the coveh extension by registering an opensbi extension
/// and init the TSMs in the platform. TSMs are currenty parsed from the device tree.
///
/// Input:
///  - fdt_address: address of FDT (Flattened Device Tree)
#[link_section = ".text"]
pub fn init(fdt_address: usize) -> i32 {
    // init at least domain 0
    let mut tsm_info = TSM_INFO.lock();

    unsafe {
        tsm_info.push_unchecked(TsmInfo {
            tsm_state: TsmState::TsmReady,
            tsm_impl_id: 69,
            tsm_version: 0,
            tsm_capabilities: 0,
            tvm_state_pages: 0,
            tvm_max_vcpus: 0,
            tvm_vcpu_state_pages: 0,
        });
    }
    // get extra domains from device tree
    let devtree = unsafe {
        let address = fdt_address as *const u8;
        DevTree::from_raw_pointer(address).unwrap()
    };

    let mut node_iter = devtree.compatible_nodes("opensbi,domain,instance");
    while let Some(node) = node_iter.next().unwrap() {
        let ret = tsm_info.push(TsmInfo {
            tsm_state: TsmState::TsmReady,
            tsm_impl_id: 0,
            tsm_version: 0,
            tsm_capabilities: 0,
            tvm_state_pages: 0,
            tvm_max_vcpus: 0,
            tvm_vcpu_state_pages: 0,
        });
    }

    // We need to register the cove host extension using the OpenSBI API.
    // The goal is to register an handler (sbi_coveh_handler) when our extension
    // is called with an ecall.
    unsafe { opensbi::sbi_ecall_register_extension(&raw mut SBI_COVE_HOST_EXTENSION) }
}

/// Retrieves the current TSM state, configuration, and supported features.
///
/// Parameters:
/// - sdid:
/// - tsm_info_address: A 4-byte aligned physical memory address where the TSM will write the TsmInfo struct.
/// - tsm_info_len: The size of the TsmInfo struct.
///
/// Returns:
/// - The number of bytes written to tsm_info_address on success.
fn sbi_covh_get_tsm_info(sdid: usize, tsm_info_address: usize, tsm_info_len: usize) -> SbiRet {
    let needed = core::mem::size_of::<TsmInfo>();
    let info = TSM_INFO.lock();

    // TODO: check if the address is valid
    if tsm_info_len < needed {
        return SbiRet {
            error: opensbi::SBI_ERR_INVALID_PARAM as isize,
            value: 0,
        };
    }

    if sdid > info.len() {
        return SbiRet {
            error: opensbi::SBI_ERR_INVALID_PARAM as isize,
            value: 0,
        };
    }

    let state = info[sdid].clone();
    let tsm_info_ptr = tsm_info_address as *mut TsmInfo;
    unsafe { tsm_info_ptr.write(state) }
    SbiRet {
        error: 0,
        value: needed as isize,
    }
}

fn sbi_covh_promote_to_tvm(
    fdt_addr: usize,
    tap_addr: usize,
    entry_sepc: usize,
    tvm_identity: usize,
) -> SbiRet {
    todo!()
}
>>>>>>> 19d52d8 (refactor: added build time jump address, debug! macro, embed-elf)
