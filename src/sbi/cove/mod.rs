/*
 * This module exposes the init function for shadowfax cove implmentation and  re-exports
 * all public symbols to ease up development.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
mod constants;
mod cove_host_extension;
mod supd_extension;
mod types;

pub use constants::*;
pub use types::*;

pub fn init(fdt_address: usize) {
    supd_extension::init();
    cove_host_extension::init(fdt_address);
}
