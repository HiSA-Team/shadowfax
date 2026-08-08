pub const DICE_INPUT_ADDR: usize = 0x8800_0000;
pub const FDT_ADDR: usize = 0x8BF0_0000;

pub mod memory_layout {
    use crate::domain::MemoryRegion;

    pub const ROOT_DOMAIN_REGIONS: [MemoryRegion; 1] = [MemoryRegion {
        base_addr: 0,
        order: 64,
        mmio: false,
        permissions: 0x3F,
    }];

    pub const UNTRUSTED_DOMAIN_REGIONS: [MemoryRegion; 4] = [
        MemoryRegion {
            base_addr: 0x8A00_0000,
            order: 25,
            mmio: false,
            permissions: 0x3F,
        },
        MemoryRegion {
            base_addr: 0x8C00_0000,
            order: 26,
            mmio: false,
            permissions: 0x3F,
        },
        MemoryRegion {
            base_addr: 0x0C00_0000,
            order: 23,
            mmio: true,
            permissions: 0x3F,
        },
        MemoryRegion {
            base_addr: 0x1000_0000,
            order: 16,
            mmio: true,
            permissions: 0x3F,
        },
    ];

    pub const TRUSTED_DOMAIN_REGIONS: [MemoryRegion; 2] = [
        MemoryRegion {
            base_addr: 0x9000_0000,
            order: 26,
            permissions: 0x3f,
            mmio: false,
        },
        MemoryRegion {
            base_addr: 0x1000_0000,
            order: 12,
            permissions: 0x3f,
            mmio: true,
        },
    ];
}
