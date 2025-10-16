#![no_std]
#![no_main]

pub mod tsm {
    pub const TSM_IMPL_ID: u32 = 0x45;
    pub const TSM_VERSION: u32 = 0x45;

    #[repr(C)]
    pub struct TsmStateData {
        pub info: TsmInfo,
    }

    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct TsmInfo {
        pub tsm_state: TsmState,
        pub tsm_impl_id: u32,
        pub tsm_version: u32,
        pub _padding: u32,
        pub tsm_capabilities: usize,
        pub tvm_state_pages: usize,
        pub tvm_max_vcpus: usize,
        pub tvm_vcpu_state_pages: usize,
    }

    pub enum TsmPageType {
        Page4k = 0,
        Page2mb = 1,
        Page1gb = 2,
        Page512gb = 3,
    }

    #[derive(Clone, Debug)]
    pub enum TsmState {
        TsmNotLoaded = 0,
        TsmLoaded = 1,
        TsmReady = 2,
    }
}

pub mod sbi {
    // CoVH constants
    pub const SBI_COVH_EXT_ID: usize = 0x434F5648;

    pub const SBI_COVH_GET_TSM_INFO: usize = 0;
    pub const SBI_COVH_CONVERT_PAGES: usize = 1;
    pub const SBI_COVH_CREATE_TVM: usize = 5;
    pub const SBI_COVH_FINALIZE_TVM: usize = 6;
    pub const SBI_COVH_DESTROY_TVM: usize = 8;
    pub const SBI_COVH_ADD_TVM_MEMORY_REGION: usize = 9;
    pub const SBI_COVH_ADD_TVM_MEASURED_PAGES: usize = 11;
    pub const SBI_COVH_CREATE_TVM_VCPU: usize = 14;
    pub const SBI_COVH_RUN_TVM_VCPU: usize = 15;

    // SUPD constants
    pub const SBI_SUPD_EXT_ID: usize = 0x53555044;
    pub const SBI_EXT_SUPD_GET_ACTIVE_DOMAINS: usize = 0;

    #[repr(C)]
    pub struct SbiRet {
        pub a0: isize,
        pub a1: isize,
    }

    pub fn sbi_call(extid: usize, fid: usize, args: &[usize; 5]) -> SbiRet {
        let (error, value);
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") extid,
                in("a6") fid,
                inlateout("a0") args[0] => error,
                inlateout("a1") args[1] => value,
                in("a2") args[2],
                in("a3") args[3],
                in("a4") args[4],
            );
        }
        SbiRet {
            a0: error,
            a1: value,
        }
    }
}
