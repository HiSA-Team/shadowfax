/*
* Global State of the TSM Driver. For now this state is an array of supervisor domains. The state
* is initialized by the init function which populates the array from the device tree. The device
* tree must declare supervisor domains with `compatible = "shadowfax,domain,config";`. Each
* supervisor domain must declare an id and a `compatible = "shadowfax,domain,instance";`.
* Other fields may be:
* - trust: a list of id of other supervisor domains that are trusted;
* - tsm-type:
*  - "none": don't load anything not a confidential supervisor domains;
*  - "default": confidential supervisor domain. Load the default TSM;
*  - "external": confidential supervisor domain. Don't load the TSN;
* - memory: an entry for the memory range used to program the PMP of the domain
*
* Note: since we use the OpenSBI implementation, the domain with id=0x0 is initialized by OpenSBI
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
use sha2::{Digest, Sha384};
use spin::mutex::Mutex;

use crate::{
    _fw_rw_start, _fw_start,
    context::Context,
    cove::TEE_SCRATCH_SIZE,
    domain::{create_confidential_domain, Domain, MemoryRegion},
};

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub struct State {
    pub domains: Vec<Domain>,
    tcb_measure: Vec<u8>,
}

impl State {
    fn new() -> Self {
        Self {
            domains: Vec::new(),
            tcb_measure: Vec::new(),
        }
    }
}

// TODO: parse domains dynamically from the device tree
// Assumption: the domain id matches with its position in the domain array
pub fn init(_fdt_addr: usize) -> Result<(), anyhow::Error> {
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new());

    // Calculate the measure of the immutable part of the firmware M-mode elements
    let tcb_digest = fw_measure();
    state.tcb_measure = tcb_digest.clone();

    let tee_stack = &raw const crate::_tee_stack_top as *const u8 as usize;

    // Create the root domain. The root domain id is always zero, so it has to be the first
    let root_domain = Domain {
        memory_regions: Vec::from([MemoryRegion {
            base_addr: 0,
            order: 64,
            mmio: false,
            permissions: 0x3F,
        }]),
        // The root domain should not be involved in Confidential call
        trust_map: 0,
        context_addr: 0,
        security_context: None,
    };
    state.domains.push(root_domain);

    // Create and add the confidential_domain
    // TODO: make this dynamic
    let context_addr = tee_stack - (TEE_SCRATCH_SIZE + size_of::<Context>()) - size_of::<Context>();
    let confidential_domain = create_confidential_domain(context_addr, tcb_digest.as_slice());

    state.domains.push(confidential_domain);

    // Create the untrusted domain
    let context_addr = context_addr - size_of::<Context>();
    let untrusted_domain = Domain {
        memory_regions: Vec::from([MemoryRegion {
            base_addr: 0x82800_0000,
            order: 24,
            mmio: false,
            permissions: 0x3F,
        }]),
        trust_map: 1 << 1,
        context_addr,
        security_context: None,
    };
    state.domains.push(untrusted_domain);

    Ok(())
}

fn fw_measure() -> Vec<u8> {
    let fw_start = &raw const _fw_start as *const u8;
    let fw_end = &raw const _fw_rw_start as *const u8;
    let fw_size = (fw_end as usize) - (fw_start as usize);
    let data = unsafe { core::slice::from_raw_parts(fw_start, fw_size) };

    Sha384::digest(data).to_vec()
}
