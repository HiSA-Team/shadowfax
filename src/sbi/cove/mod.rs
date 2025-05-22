/*
 * This module exposes the init function for shadowfax cove implmentation and  re-exports
 * all public symbols to ease up development.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */
mod constants;
mod cove_host_extension;
mod nacl_extension;
mod supd_extension;

pub use constants::*;
use cove_host_extension::SBI_COVE_HOST_EXTENSION;
use nacl_extension::SBI_NACL_EXTENSION;
use supd_extension::SBI_SUPD_EXTENSION;

use crate::opensbi;

pub fn init() {
    unsafe {
        opensbi::sbi_ecall_register_extension(&raw mut SBI_COVE_HOST_EXTENSION);
        opensbi::sbi_ecall_register_extension(&raw mut SBI_SUPD_EXTENSION);
        opensbi::sbi_ecall_register_extension(&raw mut SBI_NACL_EXTENSION);
    }
}
