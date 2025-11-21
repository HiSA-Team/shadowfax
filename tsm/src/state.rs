pub const TSM_IMPL_ID: u32 = 0x45;
pub const TSM_VERSION: u32 = 0x45;

#[repr(C)]
#[derive(Clone, Debug)]
pub struct TsmInfo {
    pub tsm_status: TsmStatus,
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
pub enum TsmStatus {
    TsmNotLoaded = 0,
    TsmLoaded = 1,
    TsmReady = 2,
}
