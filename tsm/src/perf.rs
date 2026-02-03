#[inline(always)]
pub fn read_cycle() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("csrr {}, cycle", out(reg) value);
    }
    value
}

#[inline(always)]
pub fn read_instret() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("csrr {}, instret", out(reg) value);
    }
    value
}

#[inline(always)]
pub fn read_time() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("csrr {}, time", out(reg) value);
    }
    value
}
