use crate::opensbi;

use super::{Sbiret, SUPD_EXT_ID, SUPD_EXT_NAME};

#[link_section = ".text"]
pub unsafe extern "C" fn sbi_supd_handler(
    _extid: u64,
    fid: u64,
    regs: *mut opensbi::sbi_trap_regs,
    ret: *mut opensbi::sbi_ecall_return,
) -> i32 {
    let regs = *regs;
    let mut ret = *ret;
    match fid {
        0 => {
            opensbi::sbi_printf("called sbi_supd_handler\n\0".as_ptr());
            let result = sbi_supd_get_active_domains(regs.a0);
            ret.value = result.value as u64;
            opensbi::sbi_printf("returned from sbi_supd_handler\n\0".as_ptr());

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
    let mut extension = opensbi::sbi_ecall_extension {
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
    unsafe { opensbi::sbi_ecall_register_extension(&mut extension) };
}

fn sbi_supd_get_active_domains(active_domains: u64) -> Sbiret {
    return Sbiret { error: 0, value: 0 };
}
