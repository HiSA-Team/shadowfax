use crate::domain::MemoryRegion;

pub const ROOT_DOMAIN_REGIONS: [MemoryRegion; 1] = [MemoryRegion {
    base_addr: 0,
    order: 64,
    mmio: false,
    permissions: 0x3F,
}];

pub const UNTRUSTED_DOMAIN_REGIONS: [MemoryRegion; 1] = [MemoryRegion {
    base_addr: 0x82800_0000,
    order: 24,
    mmio: false,
    permissions: 0x3F,
}];

pub const TRUSTED_DOMAIN_REGIONS: [MemoryRegion; 2] = [
    MemoryRegion {
        base_addr: 0x8200_0000,
        order: 24,
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
