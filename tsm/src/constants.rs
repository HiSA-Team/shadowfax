pub const MIN_PAGE_DIRECTORY_SIZE: usize = 16 * 1024;

/* Guest */
pub const GUEST_DRAM_SIZE: usize = 256 * 1024 * 1024;
pub const GUEST_DRAM_GPA_START: usize = 0x20_0000;
pub const GUEST_DRAM_GPA_END: usize = 0x20_0000 + GUEST_DRAM_SIZE;

pub const UART_GPA: usize = 0x1800_0000;
pub const UART_HPA: usize = 0x1000_0000;

/* Hypervisor */
pub const MAX_TVM_MEMORY_REGIONS: usize = 32;
pub const MAX_TVMS: usize = 2;
pub const MAX_HARTS: usize = 1;
pub const TVM_MAX_VCPUS: usize = 1;
