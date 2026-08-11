//! Global runtime state for the TSM driver.
//!
//! [`init`] builds this state from the platform FDT supplied by the previous
//! boot stage in register `a1`. The FDT contains an
//! `opensbi,domain,config` node under `/chosen`; its
//! `opensbi,domain,instance` subnodes define the supervisor domains, while
//! referenced `opensbi,domain,memregion` nodes define their memory access.
//!
//! OpenSBI reserves domain ID 0 for its root domain. Shadowfax mirrors that
//! root at index 0 and assigns IDs 1 onward to domain-instance nodes in device
//! tree order, matching OpenSBI's enumeration. Consequently, a domain's ID is
//! also its index in [`State::domains`]. Exactly one instance must contain the
//! standard `boot-hart` property; its `next-addr` is the next-stage entry point.
//!
//! Shadowfax adds two properties to the OpenSBI domain description:
//!
//! - `shadowfax,load-tsm` marks a domain in which Shadowfax must verify and
//!   load the embedded TSM.
//! - `shadowfax,trusts` contains domain phandles accepted by that TSM. The
//!   parser resolves those phandles into the runtime trust bitmap.
//!
//! A minimal two-domain configuration therefore has this form:
//!
//! ```text
//! opensbi-domains {
//!     compatible = "opensbi,domain,config";
//!
//!     tmem: trusted-memory {
//!         compatible = "opensbi,domain,memregion";
//!         base = <0x0 0x90000000>;
//!         order = <26>;
//!     };
//!
//!     umem: untrusted-memory {
//!         compatible = "opensbi,domain,memregion";
//!         base = <0x0 0x8a000000>;
//!         order = <25>;
//!     };
//!
//!     trusted-domain {
//!         compatible = "opensbi,domain,instance";
//!         possible-harts = <&cpu0>;
//!         regions = <&tmem 0x3f>;
//!         next-addr = <0x0 0x90000000>;
//!         next-mode = <0x1>;
//!         shadowfax,load-tsm;
//!         shadowfax,trusts = <&udomain>;
//!     };
//!
//!     udomain: untrusted-domain {
//!         compatible = "opensbi,domain,instance";
//!         possible-harts = <&cpu0>;
//!         boot-hart = <&cpu0>;
//!         regions = <&umem 0x3f>;
//!         next-addr = <0x0 0x8a000000>;
//!         next-mode = <0x1>;
//!     };
//! };
//! ```
//!
//! Author: Giuseppe Capasso <capassog97@gmail.com>

use core::cell::OnceCell;

use alloc::{string::String, vec, vec::Vec};
use common::attestation::{DiceLayer, PlatformAttestationContext};
use spin::mutex::Mutex;

use crate::{
    context::Context,
    cove::TEE_SCRATCH_SIZE,
    domain::{create_confidential_domain, Domain, MemoryRegion},
    platform::{DomainConfig, PlatformConfig},
    print_raw,
};

#[link_section = ".rodata"]
static DICE_PLATFORM_PUBLIC_KEY: &[u8; 32] = include_bytes!("../keys/root_of_trust_pub.bin");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub struct State {
    pub domains: Vec<Domain>,
    pub boot_domain_id: usize,
    pub attestation_context: PlatformAttestationContext,
    // Ongoing trusted memory: base_address, num_pages, original owner
    memory_allocations: Vec<(usize, usize, usize)>,
}

impl State {
    fn new(attestation_context: PlatformAttestationContext, boot_domain_id: usize) -> Self {
        Self {
            domains: Vec::new(),
            boot_domain_id,
            attestation_context,
            memory_allocations: Vec::new(),
        }
    }

    pub fn reclaim(&mut self, d: usize, base_addr: usize, num_pages: usize) -> anyhow::Result<()> {
        let idx = self
            .memory_allocations
            .iter()
            .enumerate()
            .position(|(_, (addr, npages, owner))| {
                *addr == base_addr && *npages == num_pages && *owner == d
            })
            .ok_or_else(|| anyhow::anyhow!("No matching memory block"))?;

        self.memory_allocations.remove(idx);
        Ok(())
    }

    pub fn track_borrow(
        &mut self,
        d: usize,
        base_addr: usize,
        num_pages: usize,
    ) -> anyhow::Result<()> {
        Ok(self.memory_allocations.push((base_addr, num_pages, d)))
    }
}

/// This function initializes the TSM-driver:
/// - read DICE input parameters, compute the new security context and create TSM CDI_ID and
/// certificate
/// - initialize the TEE stack
/// - create every supervisor domain declared by the platform device tree;
/// - load a TSM in domains marked with `shadowfax,load-tsm`.
/// Assumption: the domain id matches with its position in the domain array
pub fn init(fdt_addr: usize) -> Result<usize, anyhow::Error> {
    let platform = PlatformConfig::from_addr(fdt_addr)?;

    // First, get the security context
    let attestation_context = PlatformAttestationContext::init_from_addr(platform.dice_input_addr);
    // Verify the signature
    attestation_context.verify_with_pubkey(DICE_PLATFORM_PUBLIC_KEY)?;

    // Lock the state and init the data structure
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new(attestation_context, platform.boot_domain_id));

    let tee_stack = &raw const crate::_tee_stack_top as *const u8 as usize;

    // Create the root domain. The root domain id is always zero, so it has to be the first
    let root_domain = Domain {
        name: String::from("root"),
        memory_regions: vec![MemoryRegion {
            base_addr: 0,
            order: usize::BITS,
            mmio: false,
            permissions: 0x3f,
        }],
        // The root domain should not be involved in Confidential call
        trust_map: 0,
        next_addr: 0,
        context_addr: 0,
        has_tsm: false,
        boot_hart: false,
    };
    state.domains.push(root_domain);

    let base_context = tee_stack - (TEE_SCRATCH_SIZE + size_of::<Context>());
    for config in platform.domains {
        let context_addr = base_context - config.id * size_of::<Context>();
        let domain = if config.load_tsm {
            let tsm_context = state.attestation_context.compute_next(&[0; 32]);
            create_confidential_domain(config, context_addr, tsm_context)
        } else {
            domain_from_config(config, context_addr)
        };
        state.domains.push(domain);
    }

    dump_domains(state, platform.dice_input_addr, fdt_addr);

    Ok(state.domains[state.boot_domain_id].next_addr)
}

fn domain_from_config(config: DomainConfig, context_addr: usize) -> Domain {
    Domain {
        name: config.name,
        trust_map: config.trust_map,
        memory_regions: config.memory_regions,
        next_addr: config.next_addr,
        context_addr,
        has_tsm: false,
        boot_hart: config.boot_hart,
    }
}

fn dump_domains(state: &State, dice_input_addr: usize, fdt_addr: usize) {
    print_raw!("{:<40} : {:#010x}\n", "DICE Input Address", dice_input_addr);
    print_raw!("{:<40} : {:#010x}\n", "Boot FDT Address", fdt_addr);
    print_raw!("Supervisor domains (DT order)\n");

    for (id, domain) in state.domains.iter().enumerate() {
        print_raw!(
            "  Domain {}: {}{}{}\n",
            id,
            domain.name,
            if domain.has_tsm {
                " [trusted, load TSM]"
            } else {
                ""
            },
            if domain.boot_hart { " [boot]" } else { "" }
        );
        print_raw!(
            "    entry={:#x}, context={:#x}\n",
            domain.next_addr,
            domain.context_addr
        );

        if domain.has_tsm {
            print_raw!("    trusts=[");
            let mut separator = "";
            for trusted_id in 0..state.domains.len() {
                if domain.is_trusted(trusted_id) {
                    print_raw!(
                        "{}{}:{}",
                        separator,
                        trusted_id,
                        state.domains[trusted_id].name
                    );
                    separator = ", ";
                }
            }
            print_raw!("]\n");
        }

        for (region_id, region) in domain.memory_regions.iter().enumerate() {
            if region.order == usize::BITS {
                print_raw!(
                    "    region {}: all memory, P:{:#04x}\n",
                    region_id,
                    region.permissions
                );
                continue;
            }
            let end = region.base_addr + (1usize << region.order);
            print_raw!(
                "    region {}: {:#x}-{:#x} {}, P:{:#04x}\n",
                region_id,
                region.base_addr,
                end,
                if region.mmio { "MMIO" } else { "RAM" },
                region.permissions
            );
        }
    }
}
