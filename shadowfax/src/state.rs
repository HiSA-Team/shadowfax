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
       shadowfax-domains {
           compatible = "shadowfax,domain,config";

           trusted-domain {
               compatible = "shadowfax,domain,instance";
               id = <0x1>;
               memory = <0x0 0x81000000 0x0 0x82000000>;
           };
       };
* `
* Author: Giuseppe Capasso <capassog97@gmail.com>
*/

use core::cell::OnceCell;

use alloc::vec::Vec;
use common::tsm::{TSM_IMPL_ID, TSM_VERSION};
use fdt_rs::{base::DevTree, prelude::FallibleIterator};
use spin::mutex::Mutex;

use crate::{
    context::Context,
    cove::TEE_SCRATCH_SIZE,
    domain::{create_confidential_domain, Domain, MemoryRegion},
};

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub struct State {
    pub domains: Vec<Domain>,
}

impl State {
    fn new() -> Self {
        Self {
            domains: Vec::new(),
        }
    }
}

// TODO: parse domains dynamically from the device tree
// Assumption: the domain id matches with its position in the domain array
pub fn init(
    _fdt_addr: usize,
    tsm_state_addr: usize,
    tsm_state_size: usize,
) -> Result<(), anyhow::Error> {
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new());

    let tee_stack = &raw const crate::_tee_stack_top as *const u8 as usize;
    // Create the root domain. The root domain id is always zero, so it has to be the first

    let root_domain = Domain {
        memory_regions: Vec::from([MemoryRegion {
            base_address: 0x0,
            order: usize::MAX,
            mmio: false,
            permissions: 0x3F,
        }]),
        // The root domain should not be involved in Confidential call
        trust_map: 0,
        context_addr: 0,
        state_addr: None,
    };
    state.domains.push(root_domain);

    // Create and add the confidential_domain
    // TODO: make this dynamic
    let context_addr = tee_stack - (TEE_SCRATCH_SIZE + size_of::<Context>()) - size_of::<Context>();
    let confidential_domain = create_confidential_domain(context_addr, tsm_state_addr);

    state.domains.push(confidential_domain);

    // Create the untrusted domain
    let context_addr = context_addr - size_of::<Context>();
    let untrusted_domain = Domain {
        memory_regions: Vec::from([MemoryRegion {
            base_address: 0x824000000,
            order: 24,
            mmio: false,
            permissions: 0x3F,
        }]),
        trust_map: 1 << 1,
        context_addr: context_addr,
        state_addr: None,
    };
    state.domains.push(untrusted_domain);

    Ok(())
}
