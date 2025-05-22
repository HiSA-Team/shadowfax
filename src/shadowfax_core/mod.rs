mod attestation;
pub mod state;

pub fn init(fdt_addr: usize, next_addr: usize, next_mode: usize) {
    state::init(fdt_addr, next_addr, next_mode);
}
