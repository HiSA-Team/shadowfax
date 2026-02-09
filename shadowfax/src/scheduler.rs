use crate::state::STATE;

const CLINT_BASE: usize = 0x200_0000;
const MTIMECMP_OFFSET: usize = 0x4000;
const MTIME_OFFSET: usize = 0xbff8;

// Helper to access CLINT registers
unsafe fn clint_read(offset: usize) -> u64 {
    let addr = (CLINT_BASE + offset) as *const u64;
    addr.read_volatile()
}

unsafe fn clint_write(offset: usize, value: u64) {
    let addr = (CLINT_BASE + offset) as *mut u64;
    addr.write_volatile(value);
}

// Set the next timer interrupt
pub fn set_timer(interval_ns: u64) {
    unsafe {
        // RISC-V standard timebase is often 10MHz (check device tree in production)
        // 10MHz = 10_000_000 ticks per second
        let timebase_freq = 10_000_000;
        let ticks = (interval_ns * timebase_freq) / 1_000_000_000;

        let mtime = clint_read(MTIME_OFFSET);
        clint_write(MTIMECMP_OFFSET, mtime + ticks);

        // Enable Machine Timer Interrupt (mie.MTIE)
        // bit 7 is MTIE
        let mut mie: usize;
        core::arch::asm!("csrr {}, mie", out(reg) mie);
        mie |= 1 << 7;
        core::arch::asm!("csrw mie, {}", in(reg) mie);
    }
}
// This function should be called from your main Trap Handler
// when cause == MachineTimerInterrupt (0x8000000000000007)
#[no_mangle]
pub unsafe fn scheduler_tick() {
    debug!("timer");
}
