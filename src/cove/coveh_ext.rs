use spin::mutex::SpinMutex;

use crate::opensbi;

use super::{Sbiret, TsmInfo, TsmState, COVEH_EXT_ID};

const COVH_EXT_NAME: [u8; 8] = *b"covh\0\0\0\0";
const SHADOWFAX_IMPL_ID: u32 = 3;

const COVE_TSM_CAP_PROMOTE_TVM: usize = 0x0;
const COVE_TSM_CAP_ATTESTATION_LOCAL: usize = 0x1;
const COVE_TSM_CAP_ATTESTATION_REMOTE: usize = 0x2;
const COVE_TSM_CAP_AIA: usize = 0x3;
const COVE_TSM_CAP_MRIF: usize = 0x4;
const COVE_TSM_CAP_MEMORY_ALLOCATION: usize = 0x5;

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
        let info = SHADOWFAX_INFO.lock();
        let tsm_info_ptr = tsm_info_address as *mut TsmInfo;
        tsm_info_ptr.write(info.clone())
    }
    Sbiret {
        error: 0,
        value: needed as usize,
    }
}
