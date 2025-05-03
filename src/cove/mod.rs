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

pub use crate::cove::constants::*;
pub use crate::cove::types::*;

pub fn init() {
    supd_extension::init();
    cove_host_extension::init();
}
