pub mod cove;

/// Sbiret is a structure used to return the result of an SBI (Supervisor Binary Interface) call.
/// It contains an error code and a value, which provide information about the success or failure
/// of the call and any resulting data.
#[repr(C)]
pub struct SbiRet {
    pub error: isize,
    pub value: isize,
}

pub const SBI_COVH_GET_TSM_INFO: usize = 0;
