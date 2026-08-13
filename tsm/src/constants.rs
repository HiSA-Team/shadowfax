pub const MIN_PAGE_DIRECTORY_SIZE: usize = 16 * 1024;

pub const PTE_SIZE: usize = 8;
pub const PTE_V: u64 = 1 << 0;
pub const PTE_R: u64 = 1 << 1;
pub const PTE_W: u64 = 1 << 2;
pub const PTE_X: u64 = 1 << 3;
pub const PTE_U: u64 = 1 << 4;
pub const PTE_A: u64 = 1 << 6;
pub const PTE_D: u64 = 1 << 7;

pub const GUEST_DRAM_SIZE: usize = 256 * 1024 * 1024;
pub const GUEST_DRAM_GPA_START: usize = 0x20_0000;
pub const GUEST_DRAM_GPA_END: usize = 0x20_0000 + GUEST_DRAM_SIZE;

pub const UART_GPA: usize = 0x1800_0000;
pub const UART_HPA: usize = 0x1000_0000;

pub const MAX_TVM_MEMORY_REGIONS: usize = 32;
