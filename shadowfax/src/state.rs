/*
* Global State of the TSM Driver. For now this state is an array of supervisor domains. The state
* is initialized by the init function which populates the array from the device tree. The device
* tree must declare supervisor domains with `compatible = "opensbi,domain,config";`. Each
* supervisor domain must declare an id and a `compatible = "opensbi,domain,instance";`.
*
* Note: since we use the OpenSBI implementation, the domain with id=0 is initialized by OpenSBI
* sbi_scratch_init() function.
*
* Examples:
* `
        opensbi-domains {
            compatible = "opensbi,domain,config";

            umem: umem {
                compatible = "opensbi,domain,memregion";
                base = <0x0 0x82400000>;
                order = <24>;
            };

            tmem: tmem {
                compatible = "opensbi,domain,memregion";
                base = <0x0 0x81400000>;
                order = <23>;
            };

            tdomain: trusted-domain {
                compatible = "opensbi,domain,instance";
                possible-harts = <&cpu0>;
                regions = <&tmem 0x3f>;
                next-arg1 = <0x0 0x0>;
                next-addr = <0x0 0x81400000>;
                next-mode = <0x1>;
            };

            udomain: untrusted-domain {
                compatible = "opensbi,domain,instance";
                possible-harts = <&cpu0>;
                boot-hartid = <&cpu0>;
                regions = <&umem 0x3f>;
                next-arg1 = <0x0 0x0>;
                next-addr = <0x0 0x82400000>;
                next-mode = <0x1>;
            };

        };
* `
* Author: Giuseppe Capasso <capassog97@gmail.com>
*/

use core::cell::OnceCell;

use alloc::vec::Vec;
use common::security::AttestationContext;
use spin::mutex::Mutex;

use crate::{
    constants::{
        memory_layout::{ROOT_DOMAIN_REGIONS, UNTRUSTED_DOMAIN_REGIONS},
        DICE_INPUT_ADDR,
    },
    context::Context,
    cove::TEE_SCRATCH_SIZE,
    domain::{create_confidential_domain, Domain},
};

#[link_section = ".rodata"]
static DICE_PLATFORM_PUBLIC_KEY: &[u8; 32] = include_bytes!("../keys/root_of_trust_pub.bin");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub struct State {
    pub domains: Vec<Domain>,
    pub attestation_context: AttestationContext,
}

impl State {
    fn new() -> Self {
        Self {
            domains: Vec::new(),
            attestation_context: AttestationContext::None,
        }
    }
}

/// This function initializes the TSM-driver:
/// - read DICE input parameters, compute the new security context and create TSM CDI_ID and
/// certificate
/// - initialize the TEE stack
/// - create all domains: for now 3 hardcoded domains:
///     - Trusted domain: where the TSM code leaves
///     - Untrusted domain: normal OS/VMM
///     - Root domain: mandatory by the Supervisor Domain specification, but should never be used.
/// TODO: parse domains dynamically from the device tree
/// Assumption: the domain id matches with its position in the domain array
pub fn init(_fdt_addr: usize) -> Result<(), anyhow::Error> {
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new());

    // First, get the security context

    state.attestation_context = AttestationContext::init_from_addr(DICE_INPUT_ADDR);
    // Verify the signature
    state
        .attestation_context
        .verify(DICE_PLATFORM_PUBLIC_KEY)
        .unwrap();

    let tee_stack = &raw const crate::_tee_stack_top as *const u8 as usize;

    // Create the root domain. The root domain id is always zero, so it has to be the first
    let root_domain = Domain {
        memory_regions: Vec::from(ROOT_DOMAIN_REGIONS),
        // The root domain should not be involved in Confidential call
        trust_map: 0,
        context_addr: 0,
        has_tsm: false,
    };
    state.domains.push(root_domain);

    // Create and add the confidential_domain
    // TODO: make this dynamic
    let context_addr = tee_stack - (TEE_SCRATCH_SIZE + size_of::<Context>()) - size_of::<Context>();
    let confidential_domain = create_confidential_domain(
        context_addr,
        state.attestation_context.compute_next(&[0; 32]),
    );

    state.domains.push(confidential_domain);

    // Create the untrusted domain
    let context_addr = context_addr - size_of::<Context>();
    let untrusted_domain = Domain {
        memory_regions: Vec::from(UNTRUSTED_DOMAIN_REGIONS),
        trust_map: 1 << 1,
        context_addr,
        has_tsm: false,
    };
    state.domains.push(untrusted_domain);

    Ok(())
}
