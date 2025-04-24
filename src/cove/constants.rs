/*
 * This file contains constants definitions used by both TSM and VMMs.
 * Constants are grouped by sbi extensions. For example, at the beginning
 * there are all COVEH related extensions.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */

/*
 * COVE-H specific constants. The extension ID is used to register the
 * handler and invoke it from a VMM by loading in a7 register.
 */
pub const COVEH_EXT_ID: u64 = 0x434F5648;

/*
 * The COVEH_EXT_NAME is used to register the extension and debugging
 */
pub const COVEH_EXT_NAME: [u8; 8] = *b"covh\0\0\0\0";

/*
 * TSM specific capabilites. During initialization the TSM populates its state
 * with available capabilities. A VMM can use these values and bitwise operations
 * to understand what the TSM can do.
 */
pub const COVE_TSM_CAP_PROMOTE_TVM: usize = 0x0;
pub const COVE_TSM_CAP_ATTESTATION_LOCAL: usize = 0x1;
pub const COVE_TSM_CAP_ATTESTATION_REMOTE: usize = 0x2;
pub const COVE_TSM_CAP_AIA: usize = 0x3;
pub const COVE_TSM_CAP_MRIF: usize = 0x4;
pub const COVE_TSM_CAP_MEMORY_ALLOCATION: usize = 0x5;

/*
 * The COVE specification mandates an implementation ID for each TSM. This has to be > 2
 * since 1 is for Salus and 2 is for ACE.
 */
pub const SHADOWFAX_IMPL_ID: u32 = 69;

/*
 * This section relates to the Supervisor Domain Extension
 */
pub const SUPD_EXT_ID: u64 = 0x53555044;

/*
 * The SUPD_EXT_NAME is used to register the extension and debugging
 */
pub const SUPD_EXT_NAME: [u8; 8] = *b"supd\0\0\0\0";
