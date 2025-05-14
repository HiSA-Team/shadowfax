use crate::opensbi;

const NACL_EXT_NAME: [u8; 8] = *b"nacl\0\0\0\0";
const NACL_EXT_ID: u64 = 0x4E41434C;

#[link_section = ".data.nacl_ext"]
static mut SBI_NACL_EXTENSION: opensbi::sbi_ecall_extension = opensbi::sbi_ecall_extension {
    experimental: true,
    probe: None,
    name: NACL_EXT_NAME,
    extid_start: NACL_EXT_ID,
    extid_end: NACL_EXT_ID,
    handle: Some(sbi_nacl_handler),
    register_extensions: None,
    head: opensbi::sbi_dlist {
        next: core::ptr::null_mut(),
        prev: core::ptr::null_mut(),
    },
};

/// SBI ecall handler for NACL the extension.
///
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
#[link_section = ".text"]
pub unsafe extern "C" fn sbi_nacl_handler(
    _extid: u64,
    fid: u64,
    _regs: *mut opensbi::sbi_trap_regs,
    ret: *mut opensbi::sbi_ecall_return,
) -> i32 {
    match fid {
        _ => {
            // Unsupported function ID
            opensbi::sbi_printf("unsupported supd fid\n\0".as_ptr());
            opensbi::SBI_ENOTSUPP
        }
    }
}

/// Initialize and register the NACL extension with OpenSBI.
///
/// This must be called during SBI bring‐up to make the NACL ecall available.
/// It constructs an `sbi_ecall_extension` record and registers it.
#[link_section = ".text"]
pub fn init() -> i32 {
    // SAFETY: we trust OpenSBI to correctly link this extension into its ecall handlers
    unsafe { opensbi::sbi_ecall_register_extension(&raw mut SBI_NACL_EXTENSION) }
}
